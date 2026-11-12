//! BSP map -> `Course` (docs/reinforcement-plan.md §3, R7).
//!
//! Start and end come from the KZ mapping convention GOKZ reads: `func_button`s named
//! `climb_startbutton` / `climb_endbutton`, or `trigger_multiple` volumes named `climb_startzone` /
//! `climb_endzone`. The buttons were confirmed on a real map (kz_longjumps_v4096); the zone names come
//! from GOKZ's convention and have not been seen on a map here yet. Bonus courses (`climb_bonusN_...`)
//! are listed in the report and ignored.
//!
//! Anything the sim does not model and that can change a run's outcome (teleports, push and gravity
//! triggers, moving brushes, water, unknown triggers) goes into an **unsupported-entity report**, and the
//! import refuses unless the caller allows it. `trigger_hurt` becomes a hazard volume (a run that touches
//! one fails; damage amounts are not modeled).

use movement::trace::{EntityId, Hull, TraceWorld};
use movement::world::bsp::Entity;
use movement::world::{BspMap, Brush, World};
use movement::Vec3;

use crate::course::{Course, CourseMeta, EndSpec, StartSpec};
use crate::zone::Zone;

pub const START_BUTTON: &str = "climb_startbutton";
pub const END_BUTTON: &str = "climb_endbutton";
pub const START_ZONE: &str = "climb_startzone";
pub const END_ZONE: &str = "climb_endzone";

/// `func_button` spawnflag "Use Activates".
const SF_BUTTON_USE_ACTIVATES: u32 = 1024;

/// One kind of entity the sim does not model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unsupported {
    pub class: String,
    pub count: usize,
    /// What the real game would do that the sim does not.
    pub effect: &'static str,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportReport {
    /// How the start and end were found, e.g. "func_button climb_startbutton (entity 23)".
    pub start: Option<String>,
    pub end: Option<String>,
    /// Entities that change outcomes and are not simulated. Non-empty means the import refuses unless
    /// allowed.
    pub unsupported: Vec<Unsupported>,
    pub hazards: usize,
    /// Informational: bonus courses ignored, static brushes, and so on.
    pub notes: Vec<String>,
}

impl std::fmt::Display for ImportReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "start: {}", self.start.as_deref().unwrap_or("NOT FOUND"))?;
        writeln!(f, "end:   {}", self.end.as_deref().unwrap_or("NOT FOUND"))?;
        writeln!(f, "hazards (trigger_hurt): {}", self.hazards)?;
        if self.unsupported.is_empty() {
            writeln!(f, "unsupported: none")?;
        } else {
            writeln!(f, "unsupported (not simulated; can change a run's outcome):")?;
            for u in &self.unsupported {
                writeln!(f, "  {} x{}: {}", u.class, u.count, u.effect)?;
            }
        }
        for n in &self.notes {
            writeln!(f, "note: {n}")?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImportOptions {
    /// Import even when the report lists unsupported entities (they are then simply absent).
    pub allow_unsupported: bool,
}

/// Why an import failed, with the report so far.
#[derive(Clone, Debug)]
pub struct ImportError {
    pub reason: String,
    pub report: Box<ImportReport>,
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}\n{}", self.reason, self.report)
    }
}

fn effect_of(class: &str) -> Option<&'static str> {
    let c = class.to_ascii_lowercase();
    Some(match c.as_str() {
        "trigger_teleport" => "moves the player (checkpoints, fail resets, route sections)",
        "trigger_push" => "pushes the player",
        "trigger_gravity" => "changes gravity",
        "trigger_multiple" | "trigger_once" => "fires map logic (outputs) on touch",
        "func_door" | "func_door_rotating" | "func_movelinear" | "func_rotating" | "func_tracktrain"
        | "func_train" | "func_platrot" => "moves; the sim keeps it where it spawns",
        "func_breakable" | "func_physbox" => "can break or move; the sim keeps it solid where it spawns",
        "func_water_analog" => "water",
        _ if c.starts_with("trigger_") && c != "trigger_hurt" => "trigger with unknown effect",
        _ => return None,
    })
}

/// The pure part of the import: find start and end by name and classify every brush entity.
/// Returns (start, end, hazard entity indices, report).
pub fn classify(entities: &[Entity]) -> (Option<StartFound>, Option<EndFound>, Vec<usize>, ImportReport) {
    let mut report = ImportReport::default();
    let (mut start, mut end, mut hazards) = (None, None, Vec::new());
    let mut unsupported: Vec<Unsupported> = Vec::new();
    for (i, e) in entities.iter().enumerate() {
        let class = e.classname();
        let name = e.get("targetname").unwrap_or("").to_ascii_lowercase();
        let flags: u32 = e.get("spawnflags").and_then(|s| s.trim().parse().ok()).unwrap_or(0);
        let is_button = class.eq_ignore_ascii_case("func_button");
        let is_zone = class.eq_ignore_ascii_case("trigger_multiple");
        if name.starts_with("climb_bonus") {
            report.notes.push(format!("bonus course entity {class} {name} (entity {i}) ignored"));
            continue;
        }
        match (name.as_str(), is_button, is_zone) {
            (START_BUTTON, true, _) => {
                if flags & SF_BUTTON_USE_ACTIVATES == 0 {
                    report.notes.push(format!("start button (entity {i}) is not use-activated (spawnflags {flags})"));
                }
                start = Some(StartFound::Button(i));
                report.start = Some(format!("func_button {START_BUTTON} (entity {i})"));
                continue;
            }
            (END_BUTTON, true, _) => {
                if flags & SF_BUTTON_USE_ACTIVATES == 0 {
                    report.notes.push(format!("end button (entity {i}) is not use-activated (spawnflags {flags})"));
                }
                end = Some(EndFound::Button(i));
                report.end = Some(format!("func_button {END_BUTTON} (entity {i})"));
                continue;
            }
            (START_ZONE, _, true) => {
                start = Some(StartFound::Zone(i));
                report.start = Some(format!("trigger_multiple {START_ZONE} (entity {i})"));
                continue;
            }
            (END_ZONE, _, true) => {
                end = Some(EndFound::Zone(i));
                report.end = Some(format!("trigger_multiple {END_ZONE} (entity {i})"));
                continue;
            }
            _ => {}
        }
        if class.eq_ignore_ascii_case("trigger_hurt") {
            hazards.push(i);
            continue;
        }
        if is_button {
            report.notes.push(format!("func_button {name:?} (entity {i}) is solid and inert"));
            continue;
        }
        if let Some(effect) = effect_of(class) {
            let class = class.to_ascii_lowercase();
            match unsupported.iter_mut().find(|u| u.class == class) {
                Some(u) => u.count += 1,
                None => unsupported.push(Unsupported { class, count: 1, effect }),
            }
        }
    }
    report.hazards = hazards.len();
    report.unsupported = unsupported;
    (start, end, hazards, report)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartFound {
    Button(usize),
    Zone(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndFound {
    Button(usize),
    Zone(usize),
}

fn bounds<'a>(brushes: impl IntoIterator<Item = &'a Brush>) -> Option<(Vec3, Vec3)> {
    let mut it = brushes.into_iter();
    let first = it.next()?;
    let (mut lo, mut hi) = (first.mins, first.maxs);
    for b in it {
        lo = Vec3::new(lo.x.min(b.mins.x), lo.y.min(b.mins.y), lo.z.min(b.mins.z));
        hi = Vec3::new(hi.x.max(b.maxs.x), hi.y.max(b.maxs.y), hi.z.max(b.maxs.z));
    }
    Some((lo, hi))
}

fn center((lo, hi): (Vec3, Vec3)) -> Vec3 {
    Vec3::new((lo.x + hi.x) * 0.5, (lo.y + hi.y) * 0.5, (lo.z + hi.z) * 0.5)
}

/// Build a `Course` from a parsed map.
pub fn from_bsp(map: &BspMap, name: &str, opts: ImportOptions) -> Result<(Course, ImportReport), ImportError> {
    let (start, end, hazard_ids, mut report) = classify(&map.entities);
    if map.world.has_water() {
        report.unsupported.push(Unsupported { class: "water brushes".into(), count: 1, effect: "swimming is not simulated" });
    }
    let fail = |reason: &str, report: &ImportReport| Err(ImportError { reason: reason.into(), report: Box::new(report.clone()) });
    let (Some(start), Some(end)) = (start, end) else {
        return fail("no start or no end found by KZ naming (climb_startbutton/endbutton, climb_startzone/endzone)", &report);
    };
    if !report.unsupported.is_empty() && !opts.allow_unsupported {
        return fail("map has entities the sim does not model; pass allow_unsupported to import anyway", &report);
    }
    let zone_of = |ei: usize| -> Option<Zone> {
        let t = map.triggers.iter().find(|t| t.entity == ei)?;
        Some(Zone::from_brushes(t.brushes.iter().cloned()))
    };
    let button_bounds = |ei: usize| bounds(map.world.entity_brushes(EntityId(ei as u32)));

    let (start_spec, start_point) = match start {
        StartFound::Button(i) => {
            let Some(b) = button_bounds(i) else { return fail("start button has no solid brushes", &report) };
            (StartSpec::Button { entity: EntityId(i as u32) }, center(b))
        }
        StartFound::Zone(i) => {
            let Some(z) = zone_of(i) else { return fail("start zone has no brushes", &report) };
            let p = z.center().unwrap_or(Vec3::ZERO);
            (StartSpec::Zone(z), p)
        }
    };
    let end_spec = match end {
        EndFound::Button(i) => {
            let Some(b) = button_bounds(i) else { return fail("end button has no solid brushes", &report) };
            EndSpec::Use { entity: EntityId(i as u32), target: center(b) }
        }
        EndFound::Zone(i) => {
            let Some(z) = zone_of(i) else { return fail("end zone has no brushes", &report) };
            EndSpec::Touch(z)
        }
    };
    let hazards = hazard_ids.into_iter().filter_map(zone_of).collect();

    // Spawn: the player spawn (not a teleport destination) nearest the start.
    let spawn = map
        .spawn_points()
        .into_iter()
        .filter(|(label, ..)| label != "teleport")
        .min_by(|a, b| a.1.sub(start_point).length().total_cmp(&b.1.sub(start_point).length()));
    let Some((_, spawn_origin, spawn_yaw)) = spawn else { return fail("map has no player spawn", &report) };
    let world: World = map.world.clone().into();
    if world.point_solid(spawn_origin, Hull::STAND) {
        return fail("spawn is inside solid", &report);
    }
    let lowest = map.faces.iter().flat_map(|f| f.verts.iter().map(|v| v.z)).fold(f32::INFINITY, f32::min);
    report.notes.push(format!("spawn {spawn_origin:?} yaw {spawn_yaw}, nearest player spawn to the start"));
    let course = Course {
        world,
        spawn: (spawn_origin, spawn_yaw),
        start: start_spec,
        end: end_spec,
        hazards,
        kill_z: lowest - 256.0,
        waypoints: Vec::new(),
        meta: CourseMeta { name: name.to_string() },
    };
    Ok((course, report))
}

/// FNV-1a 64 of a file's bytes, to tie an episode to the exact map it ran on.
pub fn map_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// Read, parse, and import a `.bsp` file.
pub fn load(path: &std::path::Path, opts: ImportOptions) -> Result<(Course, ImportReport, u64), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let map = BspMap::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let name = path.file_stem().map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().into_owned());
    let (course, report) = from_bsp(&map, &name, opts).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((course, report, map_hash(&bytes)))
}
