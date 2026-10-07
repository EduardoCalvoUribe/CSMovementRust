//! The test level, described once (plan §6.3). The app renders it, `build_world` makes its collision
//! world, and the capture tooling compiles the same list into a CS:GO map (plan §9.2), so render,
//! collision and the real-game map can never disagree.
//!
//! Coordinates are Source's: x forward, y left, z up, units are game units; the floor top is z = 0.
//! Every vertex is on an integer coordinate so the compiled map has exactly the same geometry.

use movement::trace::DIST_EPSILON;
use movement::world::{Brush, Contents, PrimitiveWorld, RiseDir};
use movement::Vec3 as SVec3;


pub struct Area {
    pub name: &'static str,
    pub spawn: SVec3,
    pub yaw: f32,
}

pub struct Sign {
    pub at: SVec3,
    pub text: String,
}

pub struct Level {
    pub brushes: Vec<Brush>,
    pub signs: Vec<Sign>,
    pub areas: Vec<Area>,
}

fn cuboid(min: (f32, f32, f32), max: (f32, f32, f32)) -> Brush {
    Brush::cuboid(SVec3::new(min.0, min.1, min.2), SVec3::new(max.0, max.1, max.2))
}

fn on_top(x: f32, y: f32, z: f32) -> SVec3 {
    SVec3::new(x, y, z + DIST_EPSILON)
}

pub fn describe() -> Level {
    let mut b: Vec<Brush> = Vec::new();
    let mut signs: Vec<Sign> = Vec::new();
    let mut areas: Vec<Area> = Vec::new();
    let mut sign = |x: f32, y: f32, z: f32, t: &str| signs.push(Sign { at: SVec3::new(x, y, z), text: t.to_string() });

    // Floor with grid: friction, acceleration, counter-strafe, prestrafe.
    b.push(cuboid((-6144.0, -6144.0, -64.0), (6144.0, 6144.0, 0.0)));
    areas.push(Area { name: "Spawn / flat floor", spawn: on_top(0.0, 0.0, 0.0), yaw: 0.0 });
    sign(0.0, 200.0, 40.0, "Flat floor (64u grid)");

    // Long-jump runway with landing pads at gaps 220..290 [plan §6.3].
    let (rx0, rx1) = (1500.0, 2600.0);
    b.push(cuboid((rx0, -512.0, 0.0), (rx1, 512.0, 64.0)));
    for i in 0..8 {
        let gap = 220.0 + 10.0 * i as f32;
        let y0 = -512.0 + 128.0 * i as f32;
        b.push(cuboid((rx1 + gap, y0, 0.0), (rx1 + gap + 320.0, y0 + 128.0, 64.0)));
        sign(rx1 + gap + 40.0, y0 + 64.0, 100.0, &format!("LJ {gap}"));
    }
    areas.push(Area { name: "Long jump runway", spawn: on_top(rx0 + 64.0, 0.0, 64.0), yaw: 0.0 });

    // Bhop blocks: a row with fixed spacing, and a row with rising heights.
    for i in 0..10 {
        let x = 300.0 + 320.0 * i as f32;
        b.push(cuboid((x, 1400.0, 0.0), (x + 128.0, 1528.0, 64.0)));
        let h = 64.0 + 8.0 * i as f32;
        b.push(cuboid((x, 1800.0, 0.0), (x + 128.0, 1928.0, h)));
    }
    b.push(cuboid((-100.0, 1400.0, 0.0), (300.0, 1528.0, 64.0)));
    b.push(cuboid((-100.0, 1800.0, 0.0), (300.0, 1928.0, 64.0)));
    sign(100.0, 1464.0, 120.0, "Bhop row (gaps 192)");
    sign(100.0, 1864.0, 120.0, "Bhop ups (+8 per block)");
    areas.push(Area { name: "Bhop blocks", spawn: on_top(0.0, 1464.0, 64.0), yaw: 0.0 });

    // Ledges for jump and crouch-jump clearance [Ref §9, §10].
    for (i, h) in [18.0, 54.0, 55.0, 56.0, 57.0, 58.0, 64.0, 65.0, 66.0].into_iter().enumerate() {
        let x = 300.0 + 256.0 * i as f32;
        b.push(cuboid((x, -1600.0, 0.0), (x + 160.0, -1400.0, h)));
        sign(x + 80.0, -1500.0, h + 50.0, &format!("ledge {h}"));
    }
    areas.push(Area { name: "Ledges", spawn: on_top(150.0, -1500.0, 0.0), yaw: 0.0 });

    // Stairs: 18 and 16 unit risers, plus a 19 unit block that can't be stepped [Ref §12.3].
    for (row, riser) in [(0, 18.0), (1, 16.0)] {
        let y0 = -2400.0 - 300.0 * row as f32;
        for s in 0..8 {
            let x = 300.0 + 32.0 * s as f32;
            b.push(cuboid((x, y0, 0.0), (x + 32.0, y0 + 160.0, riser * (s + 1) as f32)));
        }
        b.push(cuboid((556.0, y0, 0.0), (800.0, y0 + 160.0, riser * 8.0)));
        sign(380.0, y0 + 80.0, riser * 8.0 + 60.0, &format!("stairs {riser}"));
    }
    b.push(cuboid((300.0, -3000.0, 0.0), (460.0, -2840.0, 19.0)));
    sign(380.0, -2920.0, 70.0, "19 (no step)");
    areas.push(Area { name: "Stairs", spawn: on_top(150.0, -2320.0, 0.0), yaw: 0.0 });

    // Free-standing wall and a corridor: wall strafe and slide clipping [Ref §12, §17].
    b.push(cuboid((-1600.0, -1024.0, 0.0), (-1568.0, 1024.0, 256.0)));
    b.push(cuboid((-2200.0, -1024.0, 0.0), (-2168.0, 1024.0, 256.0)));
    b.push(cuboid((-2072.0, -1024.0, 0.0), (-2040.0, 1024.0, 256.0)));
    sign(-1584.0, 0.0, 300.0, "Wall");
    sign(-2120.0, 0.0, 300.0, "Corridor");
    areas.push(Area { name: "Wall / corridor", spawn: on_top(-1400.0, -900.0, 0.0), yaw: 90.0 });

    // Ramps from 30 to 70 degrees with landing space [Ref §13]. Integer rises over a 256 run, within
    // 0.05 degrees of the nominal angle (46 degrees stays below the 0.7 walkable normal).
    for (i, (angle, rise)) in RAMPS.into_iter().enumerate() {
        let y = -1400.0 + 400.0 * i as f32;
        b.push(ramp((-3600.0, y, 0.0), 256.0, 256.0, rise, RiseDir::PosX));
        sign(-3472.0, y + 128.0, rise.min(256.0) + 60.0, &format!("{angle} deg"));
    }
    areas.push(Area { name: "Ramps", spawn: on_top(-3800.0, -1272.0, 0.0), yaw: 0.0 });

    // Surf ramp: a ridge of two 60 degree wedges, with a start platform above one end.
    let run = 256.0;
    b.push(ramp((-4400.0, -3000.0, 0.0), run, 4000.0, 443.0, RiseDir::PosX));
    b.push(ramp((-4400.0 + run, -3000.0, 0.0), run, 4000.0, 443.0, RiseDir::NegX));
    b.push(cuboid((-4600.0, -3400.0, 600.0), (-3800.0, -3100.0, 640.0)));
    sign(-4144.0, -3000.0, 800.0, "Surf 60");
    areas.push(Area { name: "Surf ramp", spawn: on_top(-4400.0, -3200.0, 640.0), yaw: 90.0 });

    // Floating platform with a sharp edge (edgebug), and a higher tower to fall from [Ref §14].
    b.push(cuboid((2400.0, -2600.0, 300.0), (2700.0, -2300.0, 332.0)));
    b.push(cuboid((2100.0, -2600.0, 0.0), (2300.0, -2300.0, 800.0)));
    sign(2550.0, -2450.0, 400.0, "Edgebug platform");
    areas.push(Area { name: "Edgebug tower", spawn: on_top(2200.0, -2450.0, 800.0), yaw: 0.0 });

    // Drop towers for duckbug / jumpbug [Ref §15].
    for (i, h) in [128.0, 256.0, 512.0].into_iter().enumerate() {
        let x = 3200.0 + 400.0 * i as f32;
        b.push(cuboid((x, 1400.0, 0.0), (x + 160.0, 1560.0, h)));
        sign(x + 80.0, 1480.0, h + 60.0, &format!("drop {h}"));
    }
    areas.push(Area { name: "Jumpbug drops", spawn: on_top(4080.0, 1480.0, 512.0), yaw: 270.0 });

    // Ladder: a ladder volume on the face of a wall leading to a platform [Ref §18].
    b.push(cuboid((-800.0, 2400.0, 0.0), (-640.0, 2560.0, 512.0)));
    b.push(cuboid((-804.0, 2448.0, 0.0), (-800.0, 2512.0, 512.0)).with_contents(Contents::Ladder));
    sign(-900.0, 2480.0, 200.0, "Ladder");
    areas.push(Area { name: "Ladder", spawn: on_top(-1000.0, 2480.0, 0.0), yaw: 0.0 });

    // Capture-rig additions (plan §9.5): a second wall meeting the first in a crease, a low ceiling for
    // duck cases, and a pad far from the world origin for the repeat-at-a-second-origin rule.
    b.push(cuboid((-1568.0, 1024.0, 0.0), (-1100.0, 1056.0, 256.0)));
    sign(-1400.0, 1000.0, 300.0, "Crease");
    b.push(cuboid((700.0, -900.0, 64.0), (1000.0, -700.0, 96.0)));
    sign(850.0, -800.0, 130.0, "Low ceiling (64)");
    areas.push(Area { name: "Low ceiling", spawn: on_top(550.0, -800.0, 0.0), yaw: 0.0 });
    b.push(cuboid((FAR.0 - 1024.0, FAR.1 - 1024.0, -64.0), (FAR.0 + 1024.0, FAR.1 + 1024.0, 0.0)));
    sign(FAR.0, FAR.1 + 200.0, 40.0, "Far pad");
    areas.push(Area { name: "Far pad", spawn: on_top(FAR.0, FAR.1, 0.0), yaw: 0.0 });

    Level { brushes: b, signs, areas }
}

/// Nominal ramp angle and its integer rise over a 256-unit run.
pub const RAMPS: [(f32, f32); 7] =
    [(30.0, 148.0), (40.0, 215.0), (45.0, 256.0), (46.0, 265.0), (50.0, 305.0), (60.0, 443.0), (70.0, 703.0)];

/// Centre of the pad far from the world origin [Ref §9 end].
pub const FAR: (f32, f32) = (12000.0, 12000.0);

/// Wedge occupying `low_corner + (run or width, width or run, rise)`, rising along `dir`.
fn ramp(low: (f32, f32, f32), run: f32, width: f32, rise: f32, dir: RiseDir) -> Brush {
    let (sx, sy) = match dir {
        RiseDir::PosX | RiseDir::NegX => (run, width),
        RiseDir::PosY | RiseDir::NegY => (width, run),
    };
    let min = SVec3::new(low.0, low.1, low.2);
    let max = SVec3::new(low.0 + sx, low.1 + sy, low.2 + rise);
    Brush::from_shape(movement::world::Shape::Wedge { min, max, rise: dir })
}

pub fn build_world(level: &Level) -> PrimitiveWorld {
    let mut w = PrimitiveWorld::new();
    for b in &level.brushes {
        w.add(b.clone());
    }
    w
}
