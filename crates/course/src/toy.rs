//! Seeded toy-course generator (docs/reinforcement-plan.md §8, R3).
//!
//! A toy course is a row of floating blocks along +x with nothing below them: the start platform, the
//! stage's blocks, and an end platform whose whole top is the end volume. Falling off is out of bounds.
//! Every vertex is on an integer coordinate, as in `testlevel`, so a course can later be compiled into a
//! real map for validation (plan §8, R9).
//!
//! Each course ships with a **reference solution**: commands produced by a simple hand-written controller
//! (hold forward, jump near the edge with a fresh press, release jump in the air; on `ShortBlocks` also
//! jump on the landing tick) run through the sim at generation time. The solution is a plain `UserCmd` list,
//! so it replays like a `Script` or a `Recording`, and the R3 gate replays it from scratch. Its run time is
//! the course's reference time.
//!
//! Gap ranges come from the reference controller's measured reach at 250 u/s in SimpleKZ at 128 Hz
//! (hull front edge past the takeoff edge when the origin comes back down to the landing height): 217
//! units flat, 186 at +32, 159 at +48, 260 at -64. Every range below keeps a clear margin under those.
//! "Long jumps near the mode's limit" and "bhop chains" (plan §8) need speed above 250, so they need a
//! prestrafing, air-strafing reference controller to prove them solvable, and are not here yet.
//!
//! Randomness is a local SplitMix64 seeded by the caller: same seed and stage, same course.

use movement::trace::{EntityId, DIST_EPSILON};
use movement::world::{Brush, PrimitiveWorld, World};
use movement::{process_movement, tick_interval, Buttons, ModeKind, NullObserver, PlayerState, UserCmd, Vec3};

use crate::course::{Course, CourseMeta, EndSpec, StartSpec, Waypoint};
use crate::run::{CourseRun, Status};
use crate::zone::Zone;

/// The mode and tickrate the reference solutions are produced for (plan §10: SimpleKZ, 128 Hz,
/// autobhop off).
pub const SOLUTION_MODE: ModeKind = ModeKind::SimpleKz;
pub const SOLUTION_TICKRATE: u32 = 128;
/// The reference controller gives up after this many ticks.
pub const SOLUTION_TICK_LIMIT: u32 = 128 * 60;
/// Largest yaw change per tick a reference solution uses, degrees. Equal to `env::EnvConfig`'s default
/// `max_yaw_per_tick`, so solutions replay through a default `Env` unclamped.
pub const SOLUTION_MAX_TURN: f32 = 10.0;

/// Difficulty stages, easiest first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Stage {
    /// Walk from the start platform onto the end platform; no gap.
    Walk,
    /// One gap at the start platform's height.
    Gap,
    /// Several gaps at one height.
    GapChain,
    /// Short blocks one jump apart, landings near the middle; the reference solution jumps again on the
    /// landing tick. Jumping only at edges drifts the landings forward until one overshoots, so the
    /// stage rewards a rhythm. It does not force a speed-gaining bhop (walking keeps 250 u/s); bhop
    /// chains that need speed gain wait for a strafing reference controller.
    ShortBlocks,
    /// Gaps with height changes, up and down.
    Heights,
    /// One gap of 220-250 units: beyond the 217 an unstrafed jump at 250 u/s reaches, within the ~266 an
    /// air-strafed one does. The reference solution strafes (`crate::strafe`). (Declared last so the
    /// other stages keep their seeds; `ALL` lists stages by difficulty.)
    LongJump,
}

impl Stage {
    pub const ALL: [Stage; 6] =
        [Stage::Walk, Stage::Gap, Stage::GapChain, Stage::LongJump, Stage::ShortBlocks, Stage::Heights];
}

/// How a toy course ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EndKind {
    /// The whole end platform top is the end volume.
    Touch,
    /// A button near the far end of the end platform, at eye height, pressed with `+use` (`EndSpec::Use`).
    Button,
}

/// Entity id of a toy course's end button brush (0 is the world).
pub const BUTTON_ENTITY: EntityId = EntityId(1);

/// One block of the row, by its x extent and top height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Block {
    pub min_x: f32,
    pub max_x: f32,
    pub top: f32,
}

#[derive(Clone, Debug)]
pub struct ToyCourse {
    pub course: Course,
    pub stage: Stage,
    pub seed: u64,
    /// Start platform first, end platform last.
    pub blocks: Vec<Block>,
    /// Reference controller commands, from the spawn state, in `SOLUTION_MODE` at `SOLUTION_TICKRATE`.
    pub solution: Vec<UserCmd>,
    /// The (yaw, pitch) turn on each solution command, as `env::Action` deltas (zero unless the solution
    /// strafes). Feeding them to an `Env` reproduces `solution` bit for bit.
    pub view_deltas: Vec<(f32, f32)>,
    /// Run time of the solution in ticks, or `None` if the controller failed (the R3 gate rejects that).
    pub reference_ticks: Option<u32>,
}

/// SplitMix64.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
    /// Integer in `lo..=hi`.
    fn int(&mut self, lo: i32, hi: i32) -> i32 {
        lo + (self.next() % (hi - lo + 1) as u64) as i32
    }
}

const BLOCK_DEPTH: f32 = 64.0;
const START_LEN: f32 = 384.0;
const END_LEN: f32 = 256.0;
/// Height of the start and end zones above a block top.
const ZONE_HEIGHT: f32 = 128.0;

/// A toy course with the `Touch` end.
pub fn generate(stage: Stage, seed: u64) -> ToyCourse {
    generate_with_end(stage, seed, EndKind::Touch)
}

/// A toy course with the given end. The layout depends only on `stage` and `seed`, not on `end`.
pub fn generate_with_end(stage: Stage, seed: u64, end_kind: EndKind) -> ToyCourse {
    // Mix the stage into the seed so stages with the same seed differ.
    let mut rng = Rng(seed ^ (stage as u64 + 1).wrapping_mul(0xd6e8feb86659fd93));
    let half_width = rng.int(48, 96) as f32;

    // (gap before the block, block length, top height) for each block after the start.
    let mut row: Vec<(i32, i32, i32)> = Vec::new();
    match stage {
        Stage::Walk => row.push((0, END_LEN as i32, 0)),
        Stage::Gap => row.push((rng.int(64, 170), END_LEN as i32, 0)),
        Stage::GapChain => {
            for _ in 0..rng.int(3, 6) {
                let gap = rng.int(64, 170);
                row.push((gap, rng.int(min_len(gap, 0).max(96), 256), 0));
            }
            row.push((rng.int(64, 170), END_LEN as i32, 0));
        }
        Stage::ShortBlocks => {
            // Flight from a jump at 250 u/s, takeoff origin to landing origin, is ~186 units. The first
            // block is placed so the edge jump lands near its middle; later blocks keep the pitch near the
            // flight distance so landings drift by a few units per hop.
            let len = rng.int(80, 128);
            row.push((201 - len / 2, len, 0));
            for _ in 0..rng.int(2, 5) {
                let len = rng.int(80, 128);
                let pitch = rng.int(184, 188);
                row.push((pitch - len, len, 0));
            }
            // The end platform is long, so its gap only needs to be reachable from the last landing.
            row.push((rng.int(40, 80), END_LEN as i32, 0));
        }
        Stage::LongJump => row.push((rng.int(220, 250), END_LEN as i32, 0)),
        Stage::Heights => {
            let mut top = 0;
            let n = rng.int(3, 6);
            for i in 0..=n {
                let up = rng.next() & 1 == 0;
                let (gap, rise) = if up { (rng.int(48, 110), rng.int(8, 32)) } else { (rng.int(64, 190), -rng.int(16, 96)) };
                top += rise;
                let len = if i == n { END_LEN as i32 } else { rng.int(min_len(gap, rise).max(128), 288) };
                row.push((gap, len, top));
            }
        }
    }

    let mut blocks = vec![Block { min_x: -START_LEN, max_x: 0.0, top: 0.0 }];
    for (gap, len, top) in row {
        let min_x = blocks.last().unwrap().max_x + gap as f32;
        blocks.push(Block { min_x, max_x: min_x + len as f32, top: top as f32 });
    }

    let mut world = PrimitiveWorld::new();
    for b in &blocks {
        world.add(Brush::cuboid(
            Vec3::new(b.min_x, -half_width, b.top - BLOCK_DEPTH),
            Vec3::new(b.max_x, half_width, b.top),
        ));
    }
    let start = blocks[0];
    let end = *blocks.last().unwrap();
    let lowest = blocks.iter().map(|b| b.top).fold(f32::INFINITY, f32::min);
    // The start zone is the back half of the start platform, so the run starts on the runway.
    let start_zone = Zone::cuboid(
        Vec3::new(start.min_x, -half_width, start.top - 8.0),
        Vec3::new(start.min_x + START_LEN / 2.0, half_width, start.top + ZONE_HEIGHT),
    );
    let end_spec = match end_kind {
        EndKind::Touch => EndSpec::Touch(Zone::cuboid(
            Vec3::new(end.min_x, -half_width, end.top - 8.0),
            Vec3::new(end.max_x, half_width, end.top + ZONE_HEIGHT),
        )),
        EndKind::Button => {
            // 16 deep, 32 wide, spanning eye height (64) from 32 to 80 above the platform.
            let (lo, hi) = (Vec3::new(end.max_x - 48.0, -16.0, end.top + 32.0), Vec3::new(end.max_x - 32.0, 16.0, end.top + 80.0));
            let mut b = Brush::cuboid(lo, hi);
            b.entity = BUTTON_ENTITY;
            world.add(b);
            let target = Vec3::new((lo.x + hi.x) * 0.5, 0.0, (lo.z + hi.z) * 0.5);
            EndSpec::Use { entity: BUTTON_ENTITY, target }
        }
    };
    let course = Course {
        world: World::Primitive(world),
        spawn: (Vec3::new(start.min_x + 64.0, 0.0, start.top + DIST_EPSILON), 0.0),
        start: StartSpec::Zone(start_zone),
        end: end_spec,
        hazards: Vec::new(),
        kill_z: lowest - 128.0,
        // Block-top centers, reached anywhere over the block.
        waypoints: blocks[1..]
            .iter()
            .map(|b| Waypoint { pos: Vec3::new((b.min_x + b.max_x) * 0.5, 0.0, b.top), radius: (b.max_x - b.min_x) * 0.5 + 16.0 })
            .collect(),
        meta: CourseMeta { name: format!("toy {stage:?} seed {seed}") },
    };
    let (solution, view_deltas, reference_ticks) = if stage == Stage::LongJump {
        strafe_solution(&course)
    } else {
        let (s, r) = reference_solution(&course, &blocks, stage == Stage::ShortBlocks);
        let d = vec![(0.0, 0.0); s.len()];
        (s, d, r)
    };
    ToyCourse { course, stage, seed, blocks, solution, view_deltas, reference_ticks }
}

/// Run time of `cmds` on `course` and how many commands it took, if they complete it.
pub fn evaluate(course: &Course, cmds: &[UserCmd]) -> Option<(u32, usize)> {
    let mut mode = SOLUTION_MODE.create();
    let cfg = *mode.config();
    let dt = tick_interval(SOLUTION_TICKRATE);
    let mut state = PlayerState::new(course.spawn.0);
    let mut run = CourseRun::new();
    for (i, cmd) in cmds.iter().enumerate() {
        process_movement(&cfg, mode.as_mut(), &course.world, &mut state, cmd, &mut NullObserver, dt);
        match run.update(course, &state, cmd) {
            Status::Completed => return Some((run.elapsed_ticks, i + 1)),
            Status::Hazard | Status::OutOfBounds => return None,
            Status::Idle | Status::Running => {}
        }
    }
    None
}

/// The strafing reference solution (one jump, as on `LongJump`): run forward and jump at the edge, then
/// alternate optimal air strafes. The strafe timing (first strafe length and later length) is searched,
/// keeping the fastest run that completes; after landing it holds forward (and taps `+use` on a `Use`
/// end) until done.
fn strafe_solution(course: &Course) -> (Vec<UserCmd>, Vec<(f32, f32)>, Option<u32>) {
    use crate::strafe::{Driver, Side};
    let mut d = Driver::new(&course.world, course.spawn.0, course.spawn.1);
    if !d.run_and_jump_at_edge(Buttons::FORWARD, SOLUTION_TICK_LIMIT) {
        return (d.cmds, d.deltas, None);
    }
    let use_end = matches!(course.end, EndSpec::Use { .. });
    let mut best: Option<(u32, Driver<'_, World>)> = None;
    for half in 3..=12u32 {
        for first in 1..=half {
            let mut p = d.fork();
            p.sync_strafes(Side::Left, first, half, 256);
            for i in 0..256u32 {
                let mut b = Buttons::FORWARD;
                if use_end && i % 2 == 0 {
                    b.insert(Buttons::USE);
                }
                p.step(b);
            }
            if let Some((ticks, used)) = evaluate(course, &p.cmds) {
                if best.as_ref().is_none_or(|(t, _)| ticks < *t) {
                    p.cmds.truncate(used);
                    p.deltas.truncate(used);
                    best = Some((ticks, p));
                }
            }
        }
    }
    match best {
        Some((ticks, p)) => (p.cmds, p.deltas, Some(ticks)),
        None => (d.cmds, d.deltas, None),
    }
}

/// Shortest block that a jump from the previous edge over `gap` (landing `rise` units higher) is sure to
/// land on and leave room to jump again. The landing origin is about 15.5 + flight past the edge, with
/// flight ~186 flat, shorter going up, and ~0.7 units longer per unit of drop (measured 228 at -64, 261 at
/// -128); the hull must still overlap the block, with a 24 unit margin.
fn min_len(gap: i32, rise: i32) -> i32 {
    let flight = 186 + if rise < 0 { (-rise * 7 + 9) / 10 } else { 0 };
    16 + flight - gap + 24 - 16
}

/// The reference controller, run in `SOLUTION_MODE` at `SOLUTION_TICKRATE`. It reads only the player state,
/// so its commands replay identically. Returns the commands up to the finish and the run time, or `None`
/// if it fell, hit a hazard, or ran out of ticks. On a `Use` end it walks into the button and taps `+use`
/// every other tick once on the end platform.
pub fn reference_solution(course: &Course, blocks: &[Block], jump_on_landing: bool) -> (Vec<UserCmd>, Option<u32>) {
    let mut mode = SOLUTION_MODE.create();
    let cfg = *mode.config();
    let dt = tick_interval(SOLUTION_TICKRATE);
    let mut state = PlayerState::new(course.spawn.0);
    let mut run = CourseRun::new();
    let angles = Vec3::new(0.0, course.spawn.1, 0.0);
    let mut cmds = Vec::new();
    let mut prev_jump = false;
    for tick in 0..SOLUTION_TICK_LIMIT {
        let mut buttons = Buttons::FORWARD;
        if state.on_ground() {
            let on = blocks.iter().position(|b| state.origin.x + 16.0 >= b.min_x && state.origin.x - 16.0 <= b.max_x);
            if let Some(i) = on.filter(|&i| i + 1 < blocks.len()) {
                // Jump on the last ticks before the hull leaves the edge, or at once after landing on a stage block.
                let near_edge = state.origin.x - 16.0 + state.velocity.x * dt * 2.0 >= blocks[i].max_x;
                if (near_edge || (jump_on_landing && i > 0)) && !prev_jump {
                    buttons.insert(Buttons::JUMP);
                }
            }
        }
        let on_end = state.origin.x + 16.0 >= blocks[blocks.len() - 1].min_x;
        if matches!(course.end, EndSpec::Use { .. }) && on_end && tick % 2 == 0 {
            buttons.insert(Buttons::USE);
        }
        prev_jump = buttons.contains(Buttons::JUMP);
        let cmd = UserCmd::from_buttons(tick, angles, buttons);
        process_movement(&cfg, mode.as_mut(), &course.world, &mut state, &cmd, &mut NullObserver, dt);
        cmds.push(cmd);
        match run.update(course, &state, &cmd) {
            Status::Completed => return (cmds, Some(run.elapsed_ticks)),
            Status::Hazard | Status::OutOfBounds => return (cmds, None),
            Status::Idle | Status::Running => {}
        }
    }
    (cmds, None)
}
