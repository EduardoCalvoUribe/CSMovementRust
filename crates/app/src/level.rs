//! Level loading and rendering: the test level (the default; its description lives in the `testlevel`
//! crate so the capture tooling shares it, plan §9.2) or a BSP map (M9).

use bevy::asset::RenderAssetUsages;
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use std::path::{Path, PathBuf};

use movement::trace::{Hull, TraceWorld};
use movement::world::{BspMap, Brush, Contents, RiseDir, Shape, World};
use movement::Vec3 as SVec3;
pub use testlevel::{build_world, describe, Level};

use crate::camera::to_bevy;


/// Everything spawned for the current level, so a map change can remove it.
#[derive(Component)]
pub struct LevelEntity;

/// A named place the player can be sent to (hotkeys 1-9; the first is the spawn).
#[derive(Clone, Debug)]
pub struct AreaInfo {
    pub name: String,
    pub spawn: SVec3,
    pub yaw: f32,
}

/// The level in use.
#[derive(Resource, Clone, Debug)]
pub struct CurrentLevel {
    pub name: String,
    /// The `.bsp` file, or `None` for the test level.
    pub path: Option<PathBuf>,
    pub areas: Vec<AreaInfo>,
}

pub enum Geometry {
    Test(Level),
    Bsp(Box<BspMap>),
}

pub struct LoadedLevel {
    pub info: CurrentLevel,
    pub world: World,
    pub geometry: Geometry,
}

/// Load the test level (`None`) or a BSP map.
pub fn load(path: Option<&Path>) -> Result<LoadedLevel, String> {
    let Some(path) = path else {
        let level = describe();
        let areas = level.areas.iter().map(|a| AreaInfo { name: a.name.to_string(), spawn: a.spawn, yaw: a.yaw }).collect();
        return Ok(LoadedLevel {
            info: CurrentLevel { name: "Test level".into(), path: None, areas },
            world: build_world(&level).into(),
            geometry: Geometry::Test(level),
        });
    };
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let map = BspMap::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let world: World = map.world.clone().into();
    let mut areas: Vec<AreaInfo> =
        map.spawn_points().into_iter().map(|(name, spawn, yaw)| AreaInfo { name, spawn, yaw }).collect();
    if areas.is_empty() {
        areas.push(AreaInfo { name: "map".into(), spawn: fallback_spawn(&map, &world), yaw: 0.0 });
    }
    let name = path.file_stem().map_or_else(|| path.display().to_string(), |s| s.to_string_lossy().into_owned());
    Ok(LoadedLevel {
        info: CurrentLevel { name, path: Some(path.to_path_buf()), areas },
        world,
        geometry: Geometry::Bsp(Box::new(map)),
    })
}

/// No spawn entities: stand on the first walkable face that has room for the player.
fn fallback_spawn(map: &BspMap, world: &World) -> SVec3 {
    for f in map.faces.iter().filter(|f| f.normal.z >= 0.7 && !f.displacement) {
        let c = f.verts.iter().fold(SVec3::ZERO, |a, v| a.add(*v)).scale(1.0 / f.verts.len() as f32);
        let p = c.add(SVec3::new(0.0, 0.0, 1.0));
        if !world.point_solid(p, Hull::STAND) {
            return p;
        }
    }
    SVec3::ZERO
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
    } else if n.z > 0.01 {
        [0.35, 0.55, 0.85, 1.0]
    } else if n.z >= -0.01 {
        if n.x.abs() >= n.y.abs() {
            [0.55, 0.47, 0.40, 1.0]
        } else {
            [0.48, 0.42, 0.36, 1.0]
        }
    } else {
        [0.25, 0.25, 0.28, 1.0]
    }
}

/// Polygons of a brush, each with its outward normal, in Source coordinates.
fn faces(brush: &Brush) -> Vec<(SVec3, Vec<SVec3>)> {
    let v = SVec3::new;
    match brush.shape {
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
        Shape::Convex { .. } => brush.polygons(),
    }
}

/// One mesh per brush with per-face vertex colors and world-aligned UVs (64 units per grid cell).
fn brush_mesh(brush: &Brush) -> Mesh {
    polygon_mesh(faces(brush).into_iter().map(|(n, poly)| (n, poly, face_color(n, brush.contents))))
}

/// A mesh from (outward normal, polygon, color) faces.
fn polygon_mesh(polys: impl IntoIterator<Item = (SVec3, Vec<SVec3>, [f32; 4])>) -> Mesh {
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut normals: Vec<[f32; 3]> = Vec::new();
    let mut colors: Vec<[f32; 4]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for (n, poly, color) in polys {
        // Wind each polygon counter-clockwise seen from outside, in Bevy space.
        let mut pts = poly.clone();
        let e1 = pts[1].sub(pts[0]);
        let e2 = pts[2].sub(pts[0]);
        if e1.cross(e2).dot(n) < 0.0 {
            pts.reverse();
        }
        let base = positions.len() as u32;
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
    geometry: &Geometry,
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
    match geometry {
        Geometry::Test(level) => {
            for brush in &level.brushes {
                let mat = if brush.contents == Contents::Ladder { ladder.clone() } else { solid.clone() };
                commands.spawn((Mesh3d(meshes.add(brush_mesh(brush))), MeshMaterial3d(mat), Transform::IDENTITY, LevelEntity));
            }
            for s in &level.signs {
                commands.spawn((
                    Text::new(s.text.clone()),
                    TextFont { font_size: bevy::text::FontSize::Px(16.0), ..Default::default() },
                    TextColor(Color::srgb(1.0, 0.95, 0.6)),
                    Node { position_type: PositionType::Absolute, ..Default::default() },
                    Visibility::Hidden,
                    SignLabel { world: to_bevy(s.at) },
                    LevelEntity,
                ));
            }
        }
        Geometry::Bsp(map) => {
            // Chunks keep each mesh to a manageable size and let culling work per region.
            const CHUNK: usize = 20_000;
            for chunk in map.faces.chunks(CHUNK) {
                let polys = chunk.iter().map(|f| {
                    let color = if f.displacement { displacement_color(f.normal) } else { face_color(f.normal, Contents::Solid) };
                    (f.normal, f.verts.clone(), color)
                });
                commands.spawn((Mesh3d(meshes.add(polygon_mesh(polys))), MeshMaterial3d(solid.clone()), Transform::IDENTITY, LevelEntity));
            }
            for brush in &map.ladders {
                commands.spawn((Mesh3d(meshes.add(brush_mesh(brush))), MeshMaterial3d(ladder.clone()), Transform::IDENTITY, LevelEntity));
            }
        }
    }
}

/// Displacements (terrain) in sand tones, darker when steep.
fn displacement_color(n: SVec3) -> [f32; 4] {
    if n.z >= 0.7 {
        [0.72, 0.66, 0.52, 1.0]
    } else {
        [0.55, 0.50, 0.40, 1.0]
    }
}
