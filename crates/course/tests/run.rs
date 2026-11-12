//! R0 gate (docs/reinforcement-plan.md §11): the timer and termination rules, driven by scripted commands
//! through the real movement pipeline in SimpleKZ at 128 Hz.

use course::{Course, CourseMeta, CourseRun, EndSpec, Status, Zone};
use movement::trace::DIST_EPSILON;
use movement::world::{PrimitiveWorld, World};
use movement::{process_movement, tick_interval, Buttons, ModeKind, NullObserver, PlayerState, UserCmd, Vec3};
use movement::script::Script;

const TICKRATE: u32 = 128;

fn zone(min: (f32, f32, f32), max: (f32, f32, f32)) -> Zone {
    Zone::cuboid(Vec3::new(min.0, min.1, min.2), Vec3::new(max.0, max.1, max.2))
}

/// A big flat floor; start zone x in [-64, 64], end zone x in [600, 700], both 256 units tall.
fn flat_course() -> Course {
    Course {
        world: World::Primitive(PrimitiveWorld::flat_floor(0.0)),
        spawn: (Vec3::new(0.0, 0.0, DIST_EPSILON), 0.0),
        start_zone: zone((-64.0, -64.0, -8.0), (64.0, 64.0, 256.0)),
        end: EndSpec::Touch(zone((600.0, -64.0, -8.0), (700.0, 64.0, 256.0))),
        hazards: Vec::new(),
        kill_z: -256.0,
        meta: CourseMeta { name: "flat".into() },
    }
}

/// Run `cmds`, returning the status and elapsed ticks after each command, and the end state.
fn run(course: &Course, cmds: &[UserCmd]) -> (Vec<(Status, u32)>, Vec<PlayerState>) {
    let mut mode = ModeKind::SimpleKz.create();
    let cfg = *mode.config();
    let dt = tick_interval(TICKRATE);
    let mut state = PlayerState::new(course.spawn.0);
    let mut cr = CourseRun::new();
    let (mut log, mut states) = (Vec::new(), Vec::new());
    for cmd in cmds {
        process_movement(&cfg, mode.as_mut(), &course.world, &mut state, cmd, &mut NullObserver, dt);
        cr.update(course, &state);
        log.push((cr.status, cr.elapsed_ticks));
        states.push(state.clone());
    }
    (log, states)
}

fn walk(buttons: Buttons, ticks: u32) -> Vec<UserCmd> {
    Script::new().look(0.0, 0.0).hold(buttons, ticks).build()
}

#[test]
fn idle_in_start_zone_does_not_start_the_timer() {
    let c = flat_course();
    let (log, _) = run(&c, &walk(Buttons::NONE, 200));
    assert!(log.iter().all(|&(s, t)| s == Status::Idle && t == 0));
}

#[test]
fn timer_starts_on_the_command_that_leaves_the_start_zone() {
    let c = flat_course();
    let (log, states) = run(&c, &walk(Buttons::FORWARD, 400));
    // Independent check of the first command that ends outside the start zone.
    let first_out = states.iter().position(|s| !c.start_zone.overlaps(s.origin, s.hull())).unwrap();
    assert!(first_out > 0, "starts inside the zone");
    assert_eq!(log[first_out - 1].0, Status::Idle);
    assert_eq!(log[first_out], (Status::Running, 1));
    // One tick per command afterwards, until the run finishes.
    let done = log.iter().position(|&(s, _)| s == Status::Completed).expect("reaches the end zone");
    assert_eq!(log[done].1 as usize, done - first_out + 1);
}

#[test]
fn statuses_stop_changing_after_completion() {
    let c = flat_course();
    let (log, _) = run(&c, &walk(Buttons::FORWARD, 600));
    let done = log.iter().position(|&(s, _)| s == Status::Completed).unwrap();
    let t = log[done].1;
    assert!(log[done..].iter().all(|&e| e == (Status::Completed, t)));
}

#[test]
fn returning_to_the_start_zone_resets_the_timer() {
    let c = flat_course();
    let mut cmds = walk(Buttons::FORWARD, 60);
    cmds.extend(walk(Buttons::BACK, 400));
    let (log, _) = run(&c, &cmds);
    assert!(log.iter().any(|&(s, _)| s == Status::Running));
    let last = *log.last().unwrap();
    assert_eq!(last, (Status::Idle, 0));
}

#[test]
fn cannot_finish_without_leaving_the_start_zone() {
    let mut c = flat_course();
    c.end = EndSpec::Touch(zone((-32.0, -32.0, -8.0), (32.0, 32.0, 256.0)));
    let (log, _) = run(&c, &walk(Buttons::NONE, 100));
    assert!(log.iter().all(|&(s, _)| s == Status::Idle));
}

#[test]
fn hazard_ends_the_run_and_wins_over_the_end_zone() {
    let mut c = flat_course();
    c.hazards.push(zone((300.0, -64.0, -8.0), (400.0, 64.0, 256.0)));
    let (log, _) = run(&c, &walk(Buttons::FORWARD, 600));
    let last = *log.last().unwrap();
    assert_eq!(last.0, Status::Hazard);
    // Overlapping hazard and end on the same tick resolves to the hazard.
    c.hazards = vec![zone((600.0, -64.0, -8.0), (700.0, 64.0, 256.0))];
    let (log, _) = run(&c, &walk(Buttons::FORWARD, 600));
    assert_eq!(log.last().unwrap().0, Status::Hazard);
}

#[test]
fn falling_below_kill_z_is_out_of_bounds() {
    let mut c = flat_course();
    c.world = World::Primitive(PrimitiveWorld::new());
    c.kill_z = -100.0;
    let (log, _) = run(&c, &walk(Buttons::NONE, 600));
    assert_eq!(log.last().unwrap().0, Status::OutOfBounds);
    let first = log.iter().position(|&(s, _)| s == Status::OutOfBounds).unwrap();
    assert!(log[first..].iter().all(|&(s, _)| s == Status::OutOfBounds));
}
