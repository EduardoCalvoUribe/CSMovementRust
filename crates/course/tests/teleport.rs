//! Map teleports (docs/teleport-rules.md): `Course::touch_teleports` and the command after it, against a
//! real-server capture (`compare tpprobe`, data/captures/raw/tpprobe1, not committed). The full check is
//! `compare tpprobe-check`; this pins the one behavior the sim needed a change for.

use course::{Course, CourseMeta, CourseRun, EndSpec, StartSpec, Status, Teleport, Zone};
use movement::world::{Brush, PrimitiveWorld, World};
use movement::{process_movement, tick_interval, Buttons, ModeKind, NullObserver, PlayerState, UserCmd, Vec3};

/// The probe map's floor and its lane-0 teleport (x 400..432, to 1500 0 1 facing yaw 90).
fn probe() -> Course {
    let mut w = PrimitiveWorld::new();
    w.add(Brush::cuboid(Vec3::new(-640.0, -960.0, -16.0), Vec3::new(2048.0, 1152.0, 0.0)));
    Course {
        world: World::Primitive(w),
        spawn: (Vec3::new(100.0, 0.0, 0.0), 0.0),
        start: StartSpec::Zone(Zone::cuboid(Vec3::new(0.0, -64.0, 0.0), Vec3::new(200.0, 64.0, 128.0))),
        end: EndSpec::Touch(Zone::cuboid(Vec3::new(1900.0, -64.0, 0.0), Vec3::new(1950.0, 64.0, 128.0))),
        hazards: Vec::new(),
        kill_z: -1000.0,
        waypoints: Vec::new(),
        teleports: vec![Teleport {
            zone: Zone::cuboid(Vec3::new(400.0, -64.0, 0.0), Vec3::new(432.0, 64.0, 128.0)),
            origin: Vec3::new(1500.0, 0.0, 1.0),
            angles: Some(Vec3::new(0.0, 90.0, 0.0)),
        }],
        meta: CourseMeta { name: "teleport probe".into() },
    }
}

fn run_cmd(world: &World, state: &mut PlayerState) {
    let mut mode = ModeKind::SimpleKz.create();
    let cfg = *mode.config();
    let cmd = UserCmd { tick: 0, view_angles: Vec3::ZERO, forward_move: 250.0, side_move: 0.0, up_move: 0.0, buttons: Buttons::NONE };
    process_movement(&cfg, mode.as_mut(), world, state, &cmd, &mut NullObserver, tick_interval(128));
}

/// TPa_jump: the player jumped into the trigger and arrived 1 unit above the floor still rising
/// (vz 136.37). On the server the next command starts on the ground: origin z 0.03125, vz 0, grounded
/// (capture commands 160 and 161, bit for bit). Without the re-categorization it would keep rising.
#[test]
fn the_command_after_a_teleport_categorizes_first() {
    let course = probe();
    // The state after command 160's movement, before the teleport (inside the trigger).
    let mut s = PlayerState::new(Vec3::new(410.0, 0.0, 43.0));
    s.velocity = Vec3::new(f32::from_bits(0x437a0000), 0.0, f32::from_bits(0x43085e4e));
    s.fall_velocity = -142.618_38;
    let t = course.touch_teleports(&mut s).expect("inside the trigger");
    assert_eq!(t.angles, Some(Vec3::new(0.0, 90.0, 0.0)));
    // Captured post-command 160: at the destination, velocity kept, airborne.
    assert_eq!((s.origin.x.to_bits(), s.origin.z.to_bits()), (0x44bb8000, 0x3f800000));
    assert_eq!((s.velocity.x.to_bits(), s.velocity.z.to_bits()), (0x437a0000, 0x43085e4e));
    assert!(!s.on_ground() && s.game_moved);

    let mut without = s.clone();
    without.game_moved = false;
    run_cmd(&course.world, &mut s);
    // Captured post-command 161.
    assert!(s.on_ground(), "{s:?}");
    assert_eq!((s.origin.z.to_bits(), s.velocity.z.to_bits()), (0x3d000000, 0));
    assert!(!s.game_moved, "the command clears the flag");

    run_cmd(&course.world, &mut without);
    assert!(!without.on_ground() && without.origin.z > 1.0, "without it the player keeps rising: {without:?}");
}

/// `CourseRun::after_command` applies the teleport, reports the view to snap to, and GOKZ's timer keeps
/// running through it.
#[test]
fn after_command_teleports_and_the_run_continues() {
    let course = probe();
    let mut mode = ModeKind::SimpleKz.create();
    let cfg = *mode.config();
    let dt = tick_interval(128);
    let mut s = PlayerState::new(Vec3::new(100.0, 0.0, 1.0));
    let mut run = CourseRun::starting_at(&s);
    let mut snapped = None;
    let mut k = 0;
    while snapped.is_none() && k < 1000 {
        let cmd = UserCmd { tick: k, view_angles: Vec3::ZERO, forward_move: 250.0, side_move: 0.0, up_move: 0.0, buttons: Buttons::NONE };
        process_movement(&cfg, mode.as_mut(), &course.world, &mut s, &cmd, &mut NullObserver, dt);
        snapped = run.after_command(&course, &mut s, &cmd);
        k += 1;
    }
    assert_eq!(snapped, Some(Vec3::new(0.0, 90.0, 0.0)));
    assert_eq!(s.origin, Vec3::new(1500.0, 0.0, 1.0));
    assert_eq!(run.status, Status::Running);
    let before = run.elapsed_ticks;
    let cmd = UserCmd { tick: k, ..Default::default() };
    process_movement(&cfg, mode.as_mut(), &course.world, &mut s, &cmd, &mut NullObserver, dt);
    assert_eq!(run.after_command(&course, &mut s, &cmd), None);
    assert_eq!(run.elapsed_ticks, before + 1);
}
