//! R7 (docs/reinforcement-plan.md §11): BSP -> `Course`. Classification is tested on synthetic entity
//! lists; the real-map tests use a KZ map outside the repo (never committed) at `CSMOVE_KZ_MAP`, default
//! `D:\Programs\csgo_workshop\kz\kz_longjumps_v4096.bsp`, and print SKIPPED when it is absent.

use course::bsp::{classify, from_bsp, EndFound, ImportOptions, StartFound};
use course::{use_hits, CourseRun, EndSpec, StartSpec, Status};
use movement::world::bsp::Entity;
use movement::world::BspMap;
use movement::{process_movement, tick_interval, Buttons, Hull, ModeKind, NullObserver, PlayerState, TraceWorld, UserCmd, Vec3};

fn ent(kv: &[(&str, &str)]) -> Entity {
    Entity { props: kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect() }
}

#[test]
fn classify_finds_kz_buttons_and_zones() {
    let ents = vec![
        ent(&[("classname", "worldspawn")]),
        ent(&[("classname", "func_button"), ("targetname", "climb_startbutton"), ("spawnflags", "1025")]),
        ent(&[("classname", "trigger_multiple"), ("targetname", "climb_endzone")]),
        ent(&[("classname", "trigger_hurt")]),
        ent(&[("classname", "func_button"), ("targetname", "climb_bonus1_startbutton")]),
    ];
    let (start, end, hazards, report) = classify(&ents);
    assert_eq!(start, Some(StartFound::Button(1)));
    assert_eq!(end, Some(EndFound::Zone(2)));
    assert_eq!(hazards, vec![3]);
    assert!(report.unsupported.is_empty(), "{report}");
    assert!(report.notes.iter().any(|n| n.contains("bonus")), "{report}");
}

#[test]
fn classify_reports_what_the_sim_does_not_model() {
    let ents = vec![
        ent(&[("classname", "func_button"), ("targetname", "climb_startbutton"), ("spawnflags", "1025")]),
        ent(&[("classname", "func_button"), ("targetname", "climb_endbutton"), ("spawnflags", "1")]),
        // Plain teleports are imported; these are not.
        ent(&[("classname", "trigger_teleport"), ("spawnflags", "1")]),
        ent(&[("classname", "trigger_teleport"), ("spawnflags", "1"), ("target", "missing")]),
        ent(&[("classname", "trigger_teleport"), ("spawnflags", "1"), ("target", "dest"), ("landmark", "lm")]),
        ent(&[("classname", "trigger_teleport"), ("spawnflags", "1"), ("target", "dest"), ("StartDisabled", "1")]),
        ent(&[("classname", "trigger_push")]),
        ent(&[("classname", "func_door")]),
        ent(&[("classname", "trigger_multiple"), ("targetname", "bhop_reset")]),
        ent(&[("classname", "trigger_soundscape")]),
        ent(&[("classname", "func_brush")]),
        ent(&[("classname", "info_teleport_destination"), ("targetname", "dest"), ("origin", "1 2 3"), ("angles", "0 90 0")]),
        ent(&[("classname", "trigger_teleport"), ("spawnflags", "4097"), ("target", "dest")]),
        ent(&[("classname", "trigger_teleport"), ("spawnflags", "33"), ("target", "dest")]),
        // Not for clients, or only for clients in vehicles: never touches a player on foot.
        ent(&[("classname", "trigger_teleport"), ("spawnflags", "2"), ("target", "dest")]),
    ];
    let (_, _, _, report) = classify(&ents);
    let got: Vec<(&str, usize)> = report.unsupported.iter().map(|u| (u.class.as_str(), u.count)).collect();
    assert_eq!(
        got,
        [
            ("trigger_teleport", 1),
            ("trigger_teleport", 1),
            ("trigger_teleport", 1),
            ("trigger_teleport", 1),
            ("trigger_push", 1),
            ("func_door", 1),
            ("trigger_multiple", 1),
            ("trigger_soundscape", 1)
        ]
    );
    assert_eq!(report.teleports, 1, "{report}");
    let plain = course::bsp::plain_teleport(&ents, &ents[12]).unwrap();
    assert_eq!(plain, Some((Vec3::new(1.0, 2.0, 3.0), Some(Vec3::new(0.0, 90.0, 0.0)))));
    assert_eq!(course::bsp::plain_teleport(&ents, &ents[13]).unwrap(), None, "spawnflag 32: clients in vehicles only");
    // An end button without "Use Activates" is noted.
    assert!(report.notes.iter().any(|n| n.contains("end button") && n.contains("not use-activated")), "{report}");
    let text = report.to_string();
    assert!(text.contains("trigger_teleport x1: teleport with a landmark"), "{text}");
}

fn real_map() -> Option<BspMap> {
    let path = std::env::var("CSMOVE_KZ_MAP").unwrap_or_else(|_| r"D:\Programs\csgo_workshop\kz\kz_longjumps_v4096.bsp".into());
    match std::fs::read(&path) {
        Ok(bytes) => Some(BspMap::parse(&bytes).expect("map parses")),
        Err(_) => {
            println!("SKIPPED: no KZ map at {path} (set CSMOVE_KZ_MAP)");
            None
        }
    }
}

#[test]
fn real_map_imports_its_stage_teleports() {
    let Some(map) = real_map() else { return };
    let (course, report) = from_bsp(&map, "kz", ImportOptions::default()).expect("plain teleports import by default");
    println!("{report}");
    assert!(report.unsupported.is_empty(), "{report}");
    assert_eq!((report.teleports, course.teleports.len()), (7, 7), "{report}");
    assert!(report.start.as_deref().unwrap_or("").contains("climb_startbutton"));
    assert!(report.end.as_deref().unwrap_or("").contains("climb_endbutton"));
    // Each stage teleport sends the player to the next stage's start, facing yaw 0, from an 8-unit slab.
    for t in &course.teleports {
        assert_eq!(t.angles, Some(Vec3::ZERO));
        let (lo, hi) = t.zone.bounds().expect("teleport volume");
        assert_eq!(hi.z - lo.z, 8.0);
        assert!(t.origin.z < lo.z, "destination {:?} below the slab {lo:?}", t.origin);
    }
    // Stand on a teleport slab: the player lands on the destination.
    let t = &course.teleports[0];
    let (lo, hi) = t.zone.bounds().unwrap();
    let mut s = PlayerState::new(Vec3::new((lo.x + hi.x) * 0.5, (lo.y + hi.y) * 0.5, lo.z + 1.0));
    s.velocity = Vec3::new(100.0, 0.0, 0.0);
    let hit = course.touch_teleports(&mut s).expect("touches the slab");
    assert_eq!((s.origin, s.velocity, s.ground_entity), (t.origin, Vec3::new(100.0, 0.0, 0.0), None));
    assert_eq!(hit.angles, Some(Vec3::ZERO));
}

/// A standing state on the floor in front of the button at `target`, facing it, from which `+use`
/// reaches `entity`; with the view angles to use.
fn stand_at_button(course: &course::Course, target: Vec3, entity: movement::EntityId) -> Option<(PlayerState, Vec3)> {
    let mut mode = ModeKind::SimpleKz.create();
    let cfg = *mode.config();
    for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
        for back in [24.0, 32.0, 40.0, 48.0] {
            // From button height, drop to the floor.
            let p = Vec3::new(target.x + dx * back, target.y + dy * back, target.z + 8.0);
            if course.world.point_solid(p, Hull::STAND) {
                continue;
            }
            let down = course.world.trace_hull(p, p.add(Vec3::new(0.0, 0.0, -256.0)), Hull::STAND);
            if down.fraction >= 1.0 {
                continue;
            }
            let mut s = PlayerState::new(down.end_pos);
            for t in 0..32 {
                process_movement(&cfg, mode.as_mut(), &course.world, &mut s, &UserCmd::from_buttons(t, Vec3::ZERO, Buttons::NONE), &mut NullObserver, tick_interval(128));
            }
            let eye = course::eye(&s);
            let d = target.sub(eye);
            let yaw = d.y.atan2(d.x).to_degrees();
            let pitch = -(d.z.atan2(d.length_2d())).to_degrees();
            let view = Vec3::new(pitch, yaw, 0.0);
            if s.on_ground() && use_hits(course, &s, view) == Some(entity) {
                return Some((s, view));
            }
        }
    }
    None
}

#[test]
fn real_map_imports_with_override_and_its_buttons_start_and_end_a_run() {
    let Some(map) = real_map() else { return };
    let (course, report) = from_bsp(&map, "kz_longjumps_v4096", ImportOptions { allow_unsupported: true }).unwrap();
    println!("{report}");
    let StartSpec::Button { entity: start } = course.start else { panic!("button start expected") };
    let EndSpec::Use { entity: end, target: end_target } = course.end else { panic!("button end expected") };
    assert!(!course.world.point_solid(course.spawn.0, Hull::STAND));
    let start_target = {
        let lo_hi = map.world.entity_brushes(start).fold(None, |acc: Option<(Vec3, Vec3)>, b| {
            Some(match acc {
                None => (b.mins, b.maxs),
                Some((lo, hi)) => (
                    Vec3::new(lo.x.min(b.mins.x), lo.y.min(b.mins.y), lo.z.min(b.mins.z)),
                    Vec3::new(hi.x.max(b.maxs.x), hi.y.max(b.maxs.y), hi.z.max(b.maxs.z)),
                ),
            })
        });
        let (lo, hi) = lo_hi.expect("start button brushes");
        Vec3::new((lo.x + hi.x) * 0.5, (lo.y + hi.y) * 0.5, (lo.z + hi.z) * 0.5)
    };
    // The spawn is the player spawn nearest the start button.
    assert!(course.spawn.0.sub(start_target).length() < 1024.0, "spawn {:?} far from start {start_target:?}", course.spawn.0);

    let (at_start, start_view) = stand_at_button(&course, start_target, start).expect("a spot that reaches the start button");
    let (at_end, end_view) = stand_at_button(&course, end_target, end).expect("a spot that reaches the end button");

    // Press start, idle 300 commands, then (placed at the end button: not a real run, the route itself
    // needs a human recording) press end.
    let mut run = CourseRun::starting_at(&at_start);
    let press = |view: Vec3| UserCmd::from_buttons(0, view, Buttons::USE);
    let idle = |view: Vec3| UserCmd::from_buttons(0, view, Buttons::NONE);
    // Holding +use from before reaching the button would not press; a fresh press does (command 1).
    assert_eq!(run.update(&course, &at_start, &idle(start_view)), Status::Idle);
    assert_eq!(run.update(&course, &at_start, &press(start_view)), Status::Running);
    assert_eq!(run.elapsed_ticks, 0);
    // Standing where it was pressed, GOKZ's virtual start button restarts the timer after every command
    // for 2 s (256 commands) after the press: the last restart is on command 257.
    for _ in 0..300 {
        run.update(&course, &at_start, &idle(start_view));
    }
    assert_eq!(run.elapsed_ticks, 301 - 257);
    // A press is judged from the state before its command, so first spend a command at the end button.
    run.update(&course, &at_end, &idle(end_view));
    // Pressing end while still holding +use from the previous command does not count.
    run.update(&course, &at_end, &press(end_view));
    let held = run.update(&course, &at_end, &press(end_view));
    assert_eq!(held, Status::Completed, "first press completes; the second command is after completion");
    assert_eq!(run.elapsed_ticks, 303 - 257);
}
