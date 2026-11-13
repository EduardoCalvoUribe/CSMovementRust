//! R0 gate (docs/reinforcement-plan.md §11): the timer and termination rules, driven by scripted commands
//! through the real movement pipeline in SimpleKZ at 128 Hz.

use course::{StartSpec, Course, CourseMeta, CourseRun, EndSpec, Status, Zone};
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
        start: StartSpec::Zone(zone((-64.0, -64.0, -8.0), (64.0, 64.0, 256.0))),
        end: EndSpec::Touch(zone((600.0, -64.0, -8.0), (700.0, 64.0, 256.0))),
        hazards: Vec::new(),
        kill_z: -256.0,
        waypoints: Vec::new(),
        teleports: Vec::new(),
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
        cr.update(course, &state, cmd);
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
    let first_out = states.iter().position(|s| !c.start_zone().unwrap().overlaps(s.origin, s.hull())).unwrap();
    assert!(first_out > 0, "starts inside the zone");
    assert_eq!(log[first_out - 1].0, Status::Idle);
    assert_eq!(log[first_out], (Status::Running, 1));
    // One tick per command afterwards, until the run finishes.
    let done = log.iter().position(|&(s, _)| s == Status::Completed).expect("reaches the end zone");
    assert_eq!(log[done].1 as usize, done - first_out + 1);
}

/// Pinned golden: holding forward from spawn on the flat course. The hull leaves the start zone once
/// origin x > 80 and touches the end once x >= 584, so 504 units at the 250 u/s knife run speed is
/// ~258 ticks at 128 Hz; the sim gives 259. Any change here means movement or timer rules moved.
#[test]
fn straight_walk_golden_tick_count() {
    let c = flat_course();
    let (log, _) = run(&c, &walk(Buttons::FORWARD, 600));
    let done = log.iter().position(|&(s, _)| s == Status::Completed).unwrap();
    assert_eq!(done, 312, "index of the completing command");
    assert_eq!(log[done], (Status::Completed, 259));
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
    let (log, states) = run(&c, &cmds);
    let first_run = log.iter().position(|&(s, _)| s == Status::Running).expect("leaves the start zone");
    // Independent check: the first command after leaving whose state is back inside the start zone.
    let back_in = (first_run..states.len())
        .find(|&i| c.start_zone().unwrap().overlaps(states[i].origin, states[i].hull()))
        .expect("walks back into the start zone");
    assert_eq!(log[back_in - 1].0, Status::Running);
    assert_eq!(log[back_in], (Status::Idle, 0));
    // Walking on out of the far side starts a fresh run at tick 1.
    let out_again = (back_in..states.len())
        .find(|&i| !c.start_zone().unwrap().overlaps(states[i].origin, states[i].hull()))
        .expect("leaves through the far side");
    assert_eq!(log[out_again], (Status::Running, 1));
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
