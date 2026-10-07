//! Compile the test level into a Hammer `.vmf` source so the real server runs on exactly the same brushes
//! (plan §9.2: "mirror the geometry"). Generated from `testlevel::describe`, so nothing is mirrored by
//! hand. The compiled `.bsp` is never committed (it is built with Valve's tools).

use std::fmt::Write as _;

use movement::world::{Contents, RiseDir, Shape};
use movement::Vec3;

const SOLID_MATERIAL: &str = "DEV/DEV_MEASUREGENERIC01B";
const LADDER_MATERIAL: &str = "TOOLS/TOOLSINVISIBLELADDER";
const SKY_MATERIAL: &str = "TOOLS/TOOLSSKYBOX";

/// Bot spawn points, on the flat floor away from every scenario start.
pub const SPAWNS: [(f32, f32); 4] = [(-400.0, -400.0), (-400.0, -500.0), (-500.0, -400.0), (-500.0, -500.0)];

/// Faces of a shape: outward normal and polygon (any winding).
pub fn faces(shape: Shape) -> Vec<(Vec3, Vec<Vec3>)> {
    let v = Vec3::new;
    match shape {
        Shape::Box { min: a, max: b } => vec![
            (v(0.0, 0.0, 1.0), vec![v(a.x, a.y, b.z), v(b.x, a.y, b.z), v(b.x, b.y, b.z), v(a.x, b.y, b.z)]),
            (v(0.0, 0.0, -1.0), vec![v(a.x, a.y, a.z), v(a.x, b.y, a.z), v(b.x, b.y, a.z), v(b.x, a.y, a.z)]),
            (v(1.0, 0.0, 0.0), vec![v(b.x, a.y, a.z), v(b.x, b.y, a.z), v(b.x, b.y, b.z), v(b.x, a.y, b.z)]),
            (v(-1.0, 0.0, 0.0), vec![v(a.x, a.y, a.z), v(a.x, a.y, b.z), v(a.x, b.y, b.z), v(a.x, b.y, a.z)]),
            (v(0.0, 1.0, 0.0), vec![v(a.x, b.y, a.z), v(a.x, b.y, b.z), v(b.x, b.y, b.z), v(b.x, b.y, a.z)]),
            (v(0.0, -1.0, 0.0), vec![v(a.x, a.y, a.z), v(b.x, a.y, a.z), v(b.x, a.y, b.z), v(a.x, a.y, b.z)]),
        ],
        Shape::Wedge { min: a, max: b, rise } => {
            let (lo, hi, side0, side1) = match rise {
                RiseDir::PosX => (a.x, b.x, a.y, b.y),
                RiseDir::NegX => (b.x, a.x, a.y, b.y),
                RiseDir::PosY => (a.y, b.y, a.x, b.x),
                RiseDir::NegY => (b.y, a.y, a.x, b.x),
            };
            let along_x = matches!(rise, RiseDir::PosX | RiseDir::NegX);
            let p = |u: f32, s: f32, z: f32| if along_x { v(u, s, z) } else { v(s, u, z) };
            let low0 = p(lo, side0, a.z);
            let low1 = p(lo, side1, a.z);
            let high0b = p(hi, side0, a.z);
            let high1b = p(hi, side1, a.z);
            let high0t = p(hi, side0, b.z);
            let high1t = p(hi, side1, b.z);
            // Vertex average: strictly inside the wedge (the box centre lies on the slope plane).
            let centroid = [low0, low1, high0b, high1b, high0t, high1t]
                .iter()
                .fold(Vec3::ZERO, |acc, p| acc.add(*p))
                .scale(1.0 / 6.0);
            let polys = vec![
                vec![low0, low1, high1t, high0t],
                vec![low0, high0b, high1b, low1],
                vec![high0b, high0t, high1t, high1b],
                vec![low0, high0t, high0b],
                vec![low1, high1b, high1t],
            ];
            polys
                .into_iter()
                .map(|poly| {
                    let mut n = poly[1].sub(poly[0]).cross(poly[2].sub(poly[0])).normalized();
                    // Outward: away from the centroid (wedges are convex).
                    if n.dot(poly[0].sub(centroid)) < 0.0 {
                        n = n.neg();
                    }
                    (n, poly)
                })
                .collect()
        }
        Shape::Convex { .. } => panic!("the test level is built from boxes and wedges only"),
    }
}

/// Three plane points ordered the way vbsp's `PlaneFromPoints` expects: (p0 - p1) x (p2 - p1) points out.
fn plane_points(n: Vec3, poly: &[Vec3]) -> [Vec3; 3] {
    let (p0, p1, p2) = (poly[0], poly[1], poly[2]);
    let c = p0.sub(p1).cross(p2.sub(p1));
    if c.dot(n) > 0.0 {
        [p0, p1, p2]
    } else {
        [p2, p1, p0]
    }
}

fn axes(n: Vec3) -> (&'static str, &'static str) {
    let (ax, ay, az) = (n.x.abs(), n.y.abs(), n.z.abs());
    if az >= ax && az >= ay {
        ("[1 0 0 0] 0.25", "[0 -1 0 0] 0.25")
    } else if ax >= ay {
        ("[0 1 0 0] 0.25", "[0 0 -1 0] 0.25")
    } else {
        ("[1 0 0 0] 0.25", "[0 0 -1 0] 0.25")
    }
}

fn num(f: f32) -> String {
    assert!(f == f.round(), "map vertex {f} is not on an integer coordinate");
    format!("{}", f as i64)
}

struct Ids {
    next: u32,
}

impl Ids {
    fn take(&mut self) -> u32 {
        self.next += 1;
        self.next
    }
}

fn solid(out: &mut String, ids: &mut Ids, shape: Shape, material: &str) {
    let _ = writeln!(out, "\tsolid\n\t{{\n\t\t\"id\" \"{}\"", ids.take());
    for (n, poly) in faces(shape) {
        let [a, b, c] = plane_points(n, &poly);
        let (u, v) = axes(n);
        let _ = writeln!(
            out,
            "\t\tside\n\t\t{{\n\t\t\t\"id\" \"{}\"\n\t\t\t\"plane\" \"({} {} {}) ({} {} {}) ({} {} {})\"\n\t\t\t\"material\" \"{material}\"\n\t\t\t\"uaxis\" \"{u}\"\n\t\t\t\"vaxis\" \"{v}\"\n\t\t\t\"rotation\" \"0\"\n\t\t\t\"lightmapscale\" \"16\"\n\t\t\t\"smoothing_groups\" \"0\"\n\t\t}}",
            ids.take(),
            num(a.x),
            num(a.y),
            num(a.z),
            num(b.x),
            num(b.y),
            num(b.z),
            num(c.x),
            num(c.y),
            num(c.z)
        );
    }
    out.push_str("\t}\n");
}

pub fn generate(level: &testlevel::Level) -> String {
    let mut ids = Ids { next: 1 };
    let mut out = String::new();
    out.push_str(
        "versioninfo\n{\n\t\"editorversion\" \"400\"\n\t\"editorbuild\" \"8864\"\n\t\"mapversion\" \"1\"\n\t\"formatversion\" \"100\"\n\t\"prefab\" \"0\"\n}\n\
         visgroups\n{\n}\nviewsettings\n{\n\t\"bSnapToGrid\" \"1\"\n\t\"bShowGrid\" \"1\"\n\t\"nGridSpacing\" \"64\"\n}\n",
    );
    out.push_str(
        "world\n{\n\t\"id\" \"1\"\n\t\"mapversion\" \"1\"\n\t\"classname\" \"worldspawn\"\n\t\"skyname\" \"sky_dust\"\n\t\"maxpropscreenwidth\" \"-1\"\n\t\"detailvbsp\" \"detail.vbsp\"\n\t\"detailmaterial\" \"detail/detailsprites\"\n",
    );
    let (mut lo, mut hi) = (Vec3::new(f32::MAX, f32::MAX, f32::MAX), Vec3::new(f32::MIN, f32::MIN, f32::MIN));
    for b in &level.brushes {
        let mat = if b.contents == Contents::Ladder { LADDER_MATERIAL } else { SOLID_MATERIAL };
        solid(&mut out, &mut ids, b.shape, mat);
        lo = Vec3::new(lo.x.min(b.mins.x), lo.y.min(b.mins.y), lo.z.min(b.mins.z));
        hi = Vec3::new(hi.x.max(b.maxs.x), hi.y.max(b.maxs.y), hi.z.max(b.maxs.z));
    }
    // Seal the map in a sky box so vbsp doesn't leak.
    let m = 256.0;
    let t = 64.0;
    let (a, b) = (
        Vec3::new((lo.x - m).floor(), (lo.y - m).floor(), (lo.z - m).floor()),
        Vec3::new((hi.x + m).ceil(), (hi.y + m).ceil(), (hi.z + 1024.0).ceil()),
    );
    let v = Vec3::new;
    let shell = [
        (v(a.x - t, a.y - t, a.z - t), v(b.x + t, b.y + t, a.z)),
        (v(a.x - t, a.y - t, b.z), v(b.x + t, b.y + t, b.z + t)),
        (v(a.x - t, a.y - t, a.z), v(a.x, b.y + t, b.z)),
        (v(b.x, a.y - t, a.z), v(b.x + t, b.y + t, b.z)),
        (v(a.x, a.y - t, a.z), v(b.x, a.y, b.z)),
        (v(a.x, b.y, a.z), v(b.x, b.y + t, b.z)),
    ];
    for (min, max) in shell {
        solid(&mut out, &mut ids, Shape::Box { min, max }, SKY_MATERIAL);
    }
    out.push_str("}\n");

    for (i, (x, y)) in SPAWNS.iter().enumerate() {
        let class = if i % 2 == 0 { "info_player_terrorist" } else { "info_player_counterterrorist" };
        let _ = writeln!(
            out,
            "entity\n{{\n\t\"id\" \"{}\"\n\t\"classname\" \"{class}\"\n\t\"angles\" \"0 0 0\"\n\t\"origin\" \"{} {} 1\"\n}}",
            ids.take(),
            num(*x),
            num(*y)
        );
    }
    let _ = writeln!(
        out,
        "entity\n{{\n\t\"id\" \"{}\"\n\t\"classname\" \"light_environment\"\n\t\"_light\" \"255 255 255 400\"\n\t\"_ambient\" \"200 200 210 250\"\n\t\"pitch\" \"-60\"\n\t\"angles\" \"-60 30 0\"\n\t\"origin\" \"0 0 512\"\n}}",
        ids.take()
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plane_points_face_outward() {
        let level = testlevel::describe();
        for b in &level.brushes {
            for (n, poly) in faces(b.shape) {
                let [p0, p1, p2] = plane_points(n, &poly);
                let c = p0.sub(p1).cross(p2.sub(p1)).normalized();
                assert!((c.dot(n) - 1.0).abs() < 1e-4, "{:?} face {n:?}", b.shape);
                // The brush's own bounding plane set agrees with the face normal.
                assert!(b.planes.iter().any(|p| p.normal.sub(n).length() < 1e-4), "{:?} has no plane {n:?}", b.shape);
            }
        }
    }

    #[test]
    fn generates_integer_map() {
        let text = generate(&testlevel::describe());
        assert!(text.contains("info_player_terrorist"));
        assert!(text.matches("solid\n").count() > 50);
    }
}
