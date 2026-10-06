//! The test level, described once (plan §6.3). `build_world` makes the collision world and `spawn_level`
//! makes the meshes from the same list, so render and collision can never disagree.
//!
//! Coordinates are Source's: x forward, y left, z up, units are game units; the floor top is z = 0.

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use movement::trace::DIST_EPSILON;
use movement::world::{Brush, Contents, PrimitiveWorld, RiseDir, Shape};
use movement::Vec3 as SVec3;

use crate::camera::to_bevy;

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

    // Ramps from 30 to 70 degrees with landing space [Ref §13].
    for (i, angle) in [30.0, 40.0, 45.0, 46.0, 50.0, 60.0, 70.0].into_iter().enumerate() {
        let y = -1400.0 + 400.0 * i as f32;
        b.push(Brush::ramp(SVec3::new(-3600.0, y, 0.0), 256.0, 256.0, angle, RiseDir::PosX));
        sign(-3472.0, y + 128.0, 256.0f32.min(256.0 * movement::math::deg2rad(angle).tan()) + 60.0, &format!("{angle} deg"));
    }
    areas.push(Area { name: "Ramps", spawn: on_top(-3800.0, -1272.0, 0.0), yaw: 0.0 });

    // Surf ramp: a ridge of two 60 degree wedges, with a start platform above one end.
    let run = 256.0;
    b.push(Brush::ramp(SVec3::new(-4400.0, -3000.0, 0.0), run, 4000.0, 60.0, RiseDir::PosX));
    b.push(Brush::ramp(SVec3::new(-4400.0 + run, -3000.0, 0.0), run, 4000.0, 60.0, RiseDir::NegX));
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

    Level { brushes: b, signs, areas }
}

pub fn build_world(level: &Level) -> PrimitiveWorld {
    let mut w = PrimitiveWorld::new();
    for b in &level.brushes {
        w.add(b.clone());
    }
    w
}

#[derive(Component)]
pub struct SignLabel {
    pub world: bevy::math::Vec3,
}

/// Face colors: walkable tops, steep (surfable) slopes, walls, undersides, ladders.
fn face_color(n: SVec3, contents: Contents) -> [f32; 4] {
    if contents == Contents::Ladder {
        return [1.0, 0.55, 0.1, 1.0];
    }
    if n.z >= 0.7 {
        [0.62, 0.66, 0.70, 1.0]
    } else if n.z > 0.0 {
        [0.35, 0.55, 0.85, 1.0]
    } else if n.z == 0.0 {
        if n.x != 0.0 {
            [0.55, 0.47, 0.40, 1.0]
        } else {
            [0.48, 0.42, 0.36, 1.0]
        }
    } else {
        [0.25, 0.25, 0.28, 1.0]
    }
}

/// Polygons of a shape, each with its outward normal, in Source coordinates.
fn faces(shape: Shape) -> Vec<(SVec3, Vec<SVec3>)> {
    let v = SVec3::new;
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
            // Build the +x-rising wedge in a local frame, then map it to the requested direction.
            let (lo, hi, side0, side1) = match rise {
                RiseDir::PosX => (a.x, b.x, a.y, b.y),
                RiseDir::NegX => (b.x, a.x, a.y, b.y),
                RiseDir::PosY => (a.y, b.y, a.x, b.x),
                RiseDir::NegY => (b.y, a.y, a.x, b.x),
            };
            let along_x = matches!(rise, RiseDir::PosX | RiseDir::NegX);
            // p(u, s, z): u along the rise axis, s along the other horizontal axis.
            let p = |u: f32, s: f32, z: f32| if along_x { v(u, s, z) } else { v(s, u, z) };
            let low0 = p(lo, side0, a.z);
            let low1 = p(lo, side1, a.z);
            let high0b = p(hi, side0, a.z);
            let high1b = p(hi, side1, a.z);
            let high0t = p(hi, side0, b.z);
            let high1t = p(hi, side1, b.z);
            let slope_n = {
                let e1 = high0t.sub(low0);
                let e2 = low1.sub(low0);
                let mut n = e2.cross(e1).normalized();
                if n.z < 0.0 {
                    n = n.neg();
                }
                n
            };
            let back_n = high0b.sub(low0).normalized();
            let side_n = low0.sub(low1).normalized();
            vec![
                (slope_n, vec![low0, low1, high1t, high0t]),
                (v(0.0, 0.0, -1.0), vec![low0, high0b, high1b, low1]),
                (back_n, vec![high0b, high0t, high1t, high1b]),
                (side_n, vec![low0, high0t, high0b]),
                (side_n.neg(), vec![low1, high1b, high1t]),
            ]
        }
    }
}

/// One mesh per brush with per-face vertex colors and world-aligned UVs (64 units per grid cell).
fn brush_mesh(brush: &Brush) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for (n, poly) in faces(brush.shape) {
        // Wind each polygon counter-clockwise seen from outside, in Bevy space.
        let mut pts = poly.clone();
        let e1 = pts[1].sub(pts[0]);
        let e2 = pts[2].sub(pts[0]);
        if e1.cross(e2).dot(n) < 0.0 {
            pts.reverse();
        }
        let base = positions.len() as u32;
        let color = face_color(n, brush.contents);
        for q in &pts {
            positions.push(to_bevy(*q).to_array());
            normals.push(to_bevy(n).to_array());
            colors.push(color);
            let uv = if n.z.abs() >= n.x.abs() && n.z.abs() >= n.y.abs() {
                [q.x / 64.0, q.y / 64.0]
            } else if n.x.abs() >= n.y.abs() {
                [q.y / 64.0, q.z / 64.0]
            } else {
                [q.x / 64.0, q.z / 64.0]
            };
            uvs.push(uv);
        }
        for i in 1..(pts.len() as u32 - 1) {
            // Source -> Bevy maps a right-handed frame to a right-handed frame, so winding is kept.
            indices.extend_from_slice(&[base, base + i, base + i + 1]);
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors)
        .with_inserted_indices(Indices::U32(indices))
}

/// A 64-texel tile with a darker border: one 64-unit grid cell.
fn grid_image() -> Image {
    let size = 64u32;
    let mut data = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let edge = x == 0 || y == 0;
            let mid = x == size / 2 || y == size / 2;
            let v: u8 = if edge {
                120
            } else if mid {
                200
            } else {
                235
            };
            data.extend_from_slice(&[v, v, v, 255]);
        }
    }
    let mut img = Image::new(
        Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Nearest,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..Default::default()
    });
    img
}

pub fn spawn_level(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    level: &Level,
) {
    let grid = images.add(grid_image());
    let solid = materials.add(StandardMaterial {
        base_color_texture: Some(grid),
        perceptual_roughness: 0.9,
        ..Default::default()
    });
    let ladder = materials.add(StandardMaterial {
        base_color: Color::srgba(1.0, 1.0, 1.0, 0.45),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..Default::default()
    });
    for brush in &level.brushes {
        let mat = if brush.contents == Contents::Ladder { ladder.clone() } else { solid.clone() };
        commands.spawn((Mesh3d(meshes.add(brush_mesh(brush))), MeshMaterial3d(mat), Transform::IDENTITY));
    }
    for s in &level.signs {
        commands.spawn((
            Text::new(s.text.clone()),
            TextFont { font_size: bevy::text::FontSize::Px(16.0), ..Default::default() },
            TextColor(Color::srgb(1.0, 0.95, 0.6)),
            Node { position_type: PositionType::Absolute, ..Default::default() },
            Visibility::Hidden,
            SignLabel { world: to_bevy(s.at) },
        ));
    }
}
