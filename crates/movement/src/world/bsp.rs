//! BSP maps (plan §5.4, M9): Source BSP version 19-21 (CS:GO writes 21) parsed from bytes, with a
//! `TraceWorld` that walks the node tree to candidate brushes and clips them with the primitive world's
//! per-brush routine, plus displacement collision.
//!
//! The file layout is the public Source SDK 2013 `bspfile.h`. Parsing takes a byte slice; reading the
//! file is the caller's job. Collision uses each brush's planes as the compiler wrote them, bevels
//! included, so arbitrary convex brushes are exact (docs/divergences.md D3). Not modeled: static prop
//! collision, surface properties (friction), moving brush entities, and point traces' bevel rules
//! (movement only traces hulls). Displacement collision is our own swept-box test against each triangle
//! (docs/divergences.md D18).

use crate::math::Vec3;
use crate::trace::{EntityId, Hull, TraceResult, TraceWorld};
use crate::world::primitive::{clip_box_to_planes, Brush, Plane, Ray, Shape};

const CONTENTS_SOLID: u32 = 0x1;
const CONTENTS_WINDOW: u32 = 0x2;
const CONTENTS_GRATE: u32 = 0x8;
const CONTENTS_MOVEABLE: u32 = 0x4000;
const CONTENTS_PLAYERCLIP: u32 = 0x10000;
const CONTENTS_MONSTER: u32 = 0x2000000;
const CONTENTS_LADDER: u32 = 0x20000000;
/// `MASK_PLAYERSOLID` (public `bspflags.h`).
const MASK_PLAYERSOLID: u32 =
    CONTENTS_SOLID | CONTENTS_MOVEABLE | CONTENTS_PLAYERCLIP | CONTENTS_WINDOW | CONTENTS_MONSTER | CONTENTS_GRATE;

// Texinfo flags of faces that are never drawn.
const SURF_SKY2D: i32 = 0x2;
const SURF_SKY: i32 = 0x4;
const SURF_TRIGGER: i32 = 0x10;
const SURF_NODRAW: i32 = 0x80;
const SURF_HINT: i32 = 0x100;
const SURF_SKIP: i32 = 0x200;

mod lump {
    pub const ENTITIES: usize = 0;
    pub const PLANES: usize = 1;
    pub const TEXDATA: usize = 2;
    pub const VERTEXES: usize = 3;
    pub const NODES: usize = 5;
    pub const TEXINFO: usize = 6;
    pub const FACES: usize = 7;
    pub const LEAFS: usize = 10;
    pub const EDGES: usize = 12;
    pub const SURFEDGES: usize = 13;
    pub const MODELS: usize = 14;
    pub const LEAFBRUSHES: usize = 17;
    pub const BRUSHES: usize = 18;
    pub const BRUSHSIDES: usize = 19;
    pub const DISPINFO: usize = 26;
    pub const DISP_VERTS: usize = 33;
    pub const TEXDATA_STRING_DATA: usize = 43;
    pub const TEXDATA_STRING_TABLE: usize = 44;
}

/// Little-endian reader over one lump.
#[derive(Clone, Copy)]
struct Lump<'a> {
    data: &'a [u8],
    version: i32,
}

impl<'a> Lump<'a> {
    fn count(&self, stride: usize) -> usize {
        self.data.len() / stride
    }
    fn bytes<const N: usize>(&self, at: usize) -> Result<[u8; N], String> {
        self.data.get(at..at + N).and_then(|s| s.try_into().ok()).ok_or_else(|| format!("read past lump end at {at}"))
    }
    fn i32(&self, at: usize) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.bytes(at)?))
    }
    fn u16(&self, at: usize) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.bytes(at)?))
    }
    fn i16(&self, at: usize) -> Result<i16, String> {
        Ok(i16::from_le_bytes(self.bytes(at)?))
    }
    fn f32(&self, at: usize) -> Result<f32, String> {
        Ok(f32::from_le_bytes(self.bytes(at)?))
    }
    fn vec3(&self, at: usize) -> Result<Vec3, String> {
        Ok(Vec3::new(self.f32(at)?, self.f32(at + 4)?, self.f32(at + 8)?))
    }
}

fn lumps(bytes: &[u8]) -> Result<Vec<Lump<'_>>, String> {
    if bytes.len() < 8 + 64 * 16 || &bytes[0..4] != b"VBSP" {
        return Err("not a Source BSP file (no VBSP header)".into());
    }
    let version = i32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if !(19..=21).contains(&version) {
        return Err(format!("BSP version {version} is not supported (Source BSP 19-21)"));
    }
    let mut out = Vec::with_capacity(64);
    for i in 0..64 {
        let at = 8 + 16 * i;
        let field = |k: usize| i32::from_le_bytes(bytes[at + 4 * k..at + 4 * k + 4].try_into().unwrap());
        let (ofs, len, ver) = (field(0), field(1), field(2));
        let data = bytes
            .get(ofs as usize..(ofs as usize).saturating_add(len as usize))
            .ok_or_else(|| format!("lump {i} lies outside the file"))?;
        out.push(Lump { data, version: ver });
    }
    Ok(out)
}

#[derive(Clone, Copy, Debug)]
struct Node {
    plane: usize,
    children: [i32; 2],
}

#[derive(Clone, Copy, Debug)]
struct Leaf {
    first_brush: usize,
    num_brushes: usize,
}

/// One displacement's collision triangles.
#[derive(Clone, Debug)]
struct Disp {
    mins: Vec3,
    maxs: Vec3,
    tris: Vec<[Vec3; 3]>,
}

/// Collision for a loaded map.
#[derive(Clone, Debug)]
pub struct BspWorld {
    planes: Vec<Plane>,
    nodes: Vec<Node>,
    leafs: Vec<Leaf>,
    leaf_brushes: Vec<u16>,
    /// Every brush in the file, with its raw contents. Brushes nobody collides with are kept so
    /// indices stay those of the file.
    brushes: Vec<Brush>,
    contents: Vec<u32>,
    world_head: i32,
    /// Solid brush entities (doors, walls, func_brush), moved to their entity origin.
    entity_brushes: Vec<(Brush, u32)>,
    disps: Vec<Disp>,
}

/// A face to draw, in Source coordinates.
#[derive(Clone, Debug)]
pub struct MapFace {
    pub normal: Vec3,
    pub verts: Vec<Vec3>,
    pub displacement: bool,
}

/// An entity from the entity lump: its key-value pairs in file order.
#[derive(Clone, Debug, Default)]
pub struct Entity {
    pub props: Vec<(String, String)>,
}

impl Entity {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.props.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
    }

    pub fn classname(&self) -> &str {
        self.get("classname").unwrap_or("")
    }

    pub fn origin(&self) -> Vec3 {
        self.get("origin").and_then(parse_vec3).unwrap_or(Vec3::ZERO)
    }

    /// Yaw from `angles` ("pitch yaw roll"), or the older `angle` key.
    pub fn yaw(&self) -> f32 {
        if let Some(a) = self.get("angles").and_then(parse_vec3) {
            return a.y;
        }
        self.get("angle").and_then(|s| s.trim().parse().ok()).unwrap_or(0.0)
    }

    /// Brush model index for `"model" "*N"`.
    pub fn brush_model(&self) -> Option<usize> {
        self.get("model")?.strip_prefix('*')?.parse().ok()
    }
}

fn parse_vec3(s: &str) -> Option<Vec3> {
    let mut it = s.split_whitespace().map(|t| t.parse::<f32>());
    Some(Vec3::new(it.next()?.ok()?, it.next()?.ok()?, it.next()?.ok()?))
}

/// A trigger volume from a brush entity, kept for M10 (triggers are not simulated yet).
#[derive(Clone, Debug)]
pub struct Trigger {
    pub entity: usize,
    pub brushes: Vec<Brush>,
}

/// A parsed map: collision, render faces, entities.
#[derive(Clone, Debug)]
pub struct BspMap {
    pub world: BspWorld,
    pub faces: Vec<MapFace>,
    /// Ladder volumes, which have no drawn faces.
    pub ladders: Vec<Brush>,
    pub entities: Vec<Entity>,
    pub triggers: Vec<Trigger>,
}

/// Brush entities that block the player, as long as they stay where they spawn.
const SOLID_BRUSH_ENTITIES: [&str; 9] = [
    "func_brush",
    "func_wall",
    "func_wall_toggle",
    "func_door",
    "func_door_rotating",
    "func_breakable",
    "func_button",
    "func_movelinear",
    "func_physbox",
];

impl BspMap {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let l = lumps(bytes)?;
        for (i, lump) in l.iter().enumerate() {
            if lump.data.starts_with(b"LZMA") && i != 40 {
                return Err(format!("lump {i} is LZMA-compressed, which is not supported"));
            }
        }
        let planes = parse_planes(l[lump::PLANES])?;
        let nodes = parse_nodes(l[lump::NODES])?;
        let leafs = parse_leafs(l[lump::LEAFS])?;
        let leaf_brushes =
            (0..l[lump::LEAFBRUSHES].count(2)).map(|i| l[lump::LEAFBRUSHES].u16(2 * i)).collect::<Result<Vec<_>, _>>()?;
        let (brushes, contents) = parse_brushes(l[lump::BRUSHES], l[lump::BRUSHSIDES], &planes)?;
        let models = parse_models(l[lump::MODELS])?;
        let entities = parse_entities(l[lump::ENTITIES].data);
        let world_head = models.first().ok_or("map has no world model")?.head;

        let mut world = BspWorld {
            planes,
            nodes,
            leafs,
            leaf_brushes,
            brushes,
            contents,
            world_head,
            entity_brushes: Vec::new(),
            disps: Vec::new(),
        };

        let face_planes = world.planes.clone();
        let geo = Geometry::new(&l, &face_planes)?;
        let mut faces = Vec::new();
        geo.model_faces(&models[0], Vec3::ZERO, &mut faces)?;

        // Brush entities: solid ones join collision and drawing, triggers are kept for later.
        let mut triggers = Vec::new();
        for (ei, e) in entities.iter().enumerate() {
            let Some(m) = e.brush_model().filter(|&m| m > 0 && m < models.len()) else { continue };
            let class = e.classname();
            let origin = e.origin();
            let brushes = world.model_brushes(models[m].head);
            if SOLID_BRUSH_ENTITIES.iter().any(|c| class.eq_ignore_ascii_case(c)) {
                let never_solid = class.eq_ignore_ascii_case("func_brush") && e.get("Solidity") == Some("1");
                let starts_off = e.get("StartDisabled") == Some("1");
                if never_solid || starts_off {
                    continue;
                }
                for b in brushes {
                    let c = world.contents[b];
                    world.entity_brushes.push((translated(&world.brushes[b], origin, EntityId(ei as u32)), c));
                }
                geo.model_faces(&models[m], origin, &mut faces)?;
            } else if class.starts_with("trigger_") {
                let brushes = brushes.into_iter().map(|b| translated(&world.brushes[b], origin, EntityId(ei as u32))).collect();
                triggers.push(Trigger { entity: ei, brushes });
            }
        }

        // Displacements: collision for the solid ones, drawing for all.
        for d in geo.displacements()? {
            for t in &d.tris {
                let n = t[1].sub(t[0]).cross(t[2].sub(t[0])).normalized();
                faces.push(MapFace { normal: n, verts: t.to_vec(), displacement: true });
            }
            if d.contents & MASK_PLAYERSOLID != 0 {
                world.disps.push(Disp { mins: d.mins, maxs: d.maxs, tris: d.tris });
            }
        }

        let ladders = world
            .model_brushes(world_head)
            .into_iter()
            .filter(|&b| world.contents[b] & CONTENTS_LADDER != 0)
            .map(|b| world.brushes[b].clone())
            .collect();

        Ok(Self { world, faces, ladders, entities, triggers })
    }

    /// Player spawns and teleport destinations, in that order, as (name, origin, yaw). Spawn entities
    /// sit on the floor; the origin returned is lifted until the standing hull is clear.
    pub fn spawn_points(&self) -> Vec<(String, Vec3, f32)> {
        let kinds = [
            ("info_player_counterterrorist", "CT spawn"),
            ("info_player_terrorist", "T spawn"),
            ("info_player_start", "start"),
            ("info_teleport_destination", "teleport"),
        ];
        let mut out = Vec::new();
        for (class, label) in kinds {
            for e in self.entities.iter().filter(|e| e.classname().eq_ignore_ascii_case(class)) {
                let mut o = e.origin();
                for _ in 0..64 {
                    if !self.world.point_solid(o, Hull::STAND) {
                        break;
                    }
                    o.z += 1.0;
                }
                let name = e.get("targetname").map_or_else(|| label.to_string(), |t| format!("{label} {t}"));
                out.push((name, o, e.yaw()));
            }
        }
        out
    }
}

fn translated(b: &Brush, by: Vec3, entity: EntityId) -> Brush {
    let planes = b.planes.iter().map(|p| Plane { normal: p.normal, dist: p.dist + p.normal.dot(by) }).collect();
    let (min, max) = (b.mins.add(by), b.maxs.add(by));
    let mut out = match b.shape {
        Shape::Box { .. } => Brush::cuboid(min, max),
        _ => Brush::convex(planes, min, max),
    };
    out.contents = b.contents;
    out.entity = entity;
    out
}

fn parse_planes(l: Lump) -> Result<Vec<Plane>, String> {
    (0..l.count(20)).map(|i| Ok(Plane { normal: l.vec3(20 * i)?, dist: l.f32(20 * i + 12)? })).collect()
}

fn parse_nodes(l: Lump) -> Result<Vec<Node>, String> {
    (0..l.count(32))
        .map(|i| {
            let at = 32 * i;
            Ok(Node { plane: l.i32(at)? as usize, children: [l.i32(at + 4)?, l.i32(at + 8)?] })
        })
        .collect()
}

fn parse_leafs(l: Lump) -> Result<Vec<Leaf>, String> {
    // Version 0 leaves carry ambient lighting (56 bytes); version 1 (all CS:GO maps) is 32 bytes.
    let stride = if l.version == 0 { 56 } else { 32 };
    (0..l.count(stride))
        .map(|i| {
            let at = stride * i;
            Ok(Leaf { first_brush: l.u16(at + 24)? as usize, num_brushes: l.u16(at + 26)? as usize })
        })
        .collect()
}

#[derive(Clone, Copy)]
struct Model {
    head: i32,
    first_face: usize,
    num_faces: usize,
}

fn parse_models(l: Lump) -> Result<Vec<Model>, String> {
    (0..l.count(48))
        .map(|i| {
            let at = 48 * i;
            Ok(Model { head: l.i32(at + 36)?, first_face: l.i32(at + 40)? as usize, num_faces: l.i32(at + 44)? as usize })
        })
        .collect()
}

/// Brushes with their planes in file order (bevels included). Axis-aligned six-sided brushes become
/// box brushes, which round trace end points differently (docs/divergences.md D17).
fn parse_brushes(b: Lump, s: Lump, planes: &[Plane]) -> Result<(Vec<Brush>, Vec<u32>), String> {
    let mut brushes = Vec::with_capacity(b.count(12));
    let mut contents = Vec::with_capacity(b.count(12));
    for i in 0..b.count(12) {
        let first = b.i32(12 * i)? as usize;
        let n = b.i32(12 * i + 4)? as usize;
        let c = b.i32(12 * i + 8)? as u32;
        let mut ps = Vec::with_capacity(n);
        for k in first..first + n {
            let pi = s.u16(8 * k)? as usize;
            ps.push(*planes.get(pi).ok_or_else(|| format!("brush side {k} names plane {pi}"))?);
        }
        let axis = |p: &Plane| [p.normal.x, p.normal.y, p.normal.z].iter().filter(|v| **v != 0.0).count() == 1;
        let (mut min, mut max) = (Vec3::new(-65536.0, -65536.0, -65536.0), Vec3::new(65536.0, 65536.0, 65536.0));
        for p in ps.iter().filter(|p| axis(p)) {
            for k in 0..3 {
                let nk = p.normal.get(k);
                if nk == 1.0 {
                    max.set(k, p.dist);
                } else if nk == -1.0 {
                    min.set(k, -p.dist);
                }
            }
        }
        let brush = if ps.len() == 6 && ps.iter().all(axis) { Brush::cuboid(min, max) } else { Brush::convex(ps, min, max) };
        let brush = if c & CONTENTS_LADDER != 0 { brush.with_contents(crate::world::Contents::Ladder) } else { brush };
        brushes.push(brush);
        contents.push(c);
    }
    Ok((brushes, contents))
}

fn parse_entities(data: &[u8]) -> Vec<Entity> {
    let text = String::from_utf8_lossy(data);
    let mut out = Vec::new();
    let mut cur: Option<Entity> = None;
    let mut chars = text.chars().peekable();
    let mut pending_key: Option<String> = None;
    while let Some(c) = chars.next() {
        match c {
            '{' => cur = Some(Entity::default()),
            '}' => {
                if let Some(e) = cur.take() {
                    out.push(e);
                }
                pending_key = None;
            }
            '"' => {
                let mut s = String::new();
                for ch in chars.by_ref() {
                    if ch == '"' {
                        break;
                    }
                    s.push(ch);
                }
                if let Some(e) = cur.as_mut() {
                    match pending_key.take() {
                        Some(k) => e.props.push((k, s)),
                        None => pending_key = Some(s),
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Face, vertex and displacement lumps, for drawing and displacement collision.
struct Geometry<'a> {
    planes: &'a [Plane],
    verts: Vec<Vec3>,
    edges: Lump<'a>,
    surfedges: Lump<'a>,
    faces: Lump<'a>,
    texinfo: Lump<'a>,
    texdata: Lump<'a>,
    tex_table: Lump<'a>,
    tex_strings: &'a [u8],
    dispinfo: Lump<'a>,
    disp_verts: Lump<'a>,
}

struct DispSurface {
    contents: u32,
    mins: Vec3,
    maxs: Vec3,
    tris: Vec<[Vec3; 3]>,
}

impl<'a> Geometry<'a> {
    fn new(l: &[Lump<'a>], planes: &'a [Plane]) -> Result<Self, String> {
        let v = l[lump::VERTEXES];
        Ok(Self {
            planes,
            verts: (0..v.count(12)).map(|i| v.vec3(12 * i)).collect::<Result<_, _>>()?,
            edges: l[lump::EDGES],
            surfedges: l[lump::SURFEDGES],
            faces: l[lump::FACES],
            texinfo: l[lump::TEXINFO],
            texdata: l[lump::TEXDATA],
            tex_table: l[lump::TEXDATA_STRING_TABLE],
            tex_strings: l[lump::TEXDATA_STRING_DATA].data,
            dispinfo: l[lump::DISPINFO],
            disp_verts: l[lump::DISP_VERTS],
        })
    }

    fn face_verts(&self, f: usize) -> Result<Vec<Vec3>, String> {
        let at = 56 * f;
        let first = self.faces.i32(at + 4)? as usize;
        let n = self.faces.i16(at + 8)? as usize;
        let mut out = Vec::with_capacity(n);
        for k in first..first + n {
            let e = self.surfedges.i32(4 * k)?;
            let ei = e.unsigned_abs() as usize;
            let vi = if e >= 0 { self.edges.u16(4 * ei)? } else { self.edges.u16(4 * ei + 2)? } as usize;
            out.push(*self.verts.get(vi).ok_or("edge names a missing vertex")?);
        }
        Ok(out)
    }

    /// Outward normal of a face: its plane, flipped when the face is on the plane's back side.
    fn face_normal(&self, f: usize) -> Result<Vec3, String> {
        let plane = self.planes.get(self.faces.u16(56 * f)? as usize).ok_or("face names a missing plane")?;
        let side = self.faces.bytes::<1>(56 * f + 2)?[0];
        Ok(if side != 0 { plane.normal.neg() } else { plane.normal })
    }

    fn texture_name(&self, texdata: i32) -> String {
        let name = || -> Result<String, String> {
            let id = self.texdata.i32(32 * texdata as usize + 12)? as usize;
            let ofs = self.tex_table.i32(4 * id)? as usize;
            let s = self.tex_strings.get(ofs..).ok_or("bad texture name offset")?;
            let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
            Ok(String::from_utf8_lossy(&s[..end]).into_owned())
        };
        name().unwrap_or_default()
    }

    /// Whether a face is drawn: not sky, trigger, nodraw, hint or skip, and no tool texture.
    fn drawn(&self, f: usize) -> Result<bool, String> {
        let ti = self.faces.i16(56 * f + 10)?;
        if ti < 0 {
            return Ok(false);
        }
        let flags = self.texinfo.i32(72 * ti as usize + 64)?;
        if flags & (SURF_SKY2D | SURF_SKY | SURF_TRIGGER | SURF_NODRAW | SURF_HINT | SURF_SKIP) != 0 {
            return Ok(false);
        }
        let td = self.texinfo.i32(72 * ti as usize + 68)?;
        Ok(!self.texture_name(td).to_ascii_uppercase().starts_with("TOOLS/"))
    }

    fn model_faces(&self, m: &Model, by: Vec3, out: &mut Vec<MapFace>) -> Result<(), String> {
        for f in m.first_face..m.first_face + m.num_faces {
            // Displacement faces are drawn from the displacement itself.
            if self.faces.i16(56 * f + 12)? >= 0 || !self.drawn(f)? {
                continue;
            }
            let verts: Vec<Vec3> = self.face_verts(f)?.into_iter().map(|v| v.add(by)).collect();
            if verts.len() < 3 {
                continue;
            }
            out.push(MapFace { normal: self.face_normal(f)?, verts, displacement: false });
        }
        Ok(())
    }

    /// Displacement surfaces: the base face's corners, starting at the one nearest `startPosition`,
    /// interpolated into a `(2^power + 1)²` grid, each vertex offset along its stored vector.
    fn displacements(&self) -> Result<Vec<DispSurface>, String> {
        let mut out = Vec::new();
        for i in 0..self.dispinfo.count(176) {
            let at = 176 * i;
            let start = self.dispinfo.vec3(at)?;
            let first_vert = self.dispinfo.i32(at + 12)? as usize;
            let power = self.dispinfo.i32(at + 20)?;
            let contents = self.dispinfo.i32(at + 32)? as u32;
            let face = self.dispinfo.u16(at + 36)? as usize;
            let corners = self.face_verts(face)?;
            if corners.len() != 4 || !(1..=4).contains(&power) {
                continue;
            }
            let dist2 = |p: Vec3| p.sub(start).dot(p.sub(start));
            let s = (0..4).min_by(|&a, &b| dist2(corners[a]).total_cmp(&dist2(corners[b]))).unwrap();
            let c: Vec<Vec3> = (0..4).map(|k| corners[(s + k) % 4]).collect();
            let n = (1usize << power) + 1;
            let lerp = |a: Vec3, b: Vec3, t: f32| a.add(b.sub(a).scale(t));
            let mut grid = Vec::with_capacity(n * n);
            for y in 0..n {
                let ty = y as f32 / (n - 1) as f32;
                let left = lerp(c[0], c[1], ty);
                let right = lerp(c[3], c[2], ty);
                for x in 0..n {
                    let base = lerp(left, right, x as f32 / (n - 1) as f32);
                    let vat = 20 * (first_vert + y * n + x);
                    let dir = self.disp_verts.vec3(vat)?;
                    let d = self.disp_verts.f32(vat + 12)?;
                    grid.push(base.add(dir.scale(d)));
                }
            }
            // Wind every triangle so its normal faces the same way as the base face's.
            let up = self.face_normal(face)?;
            let mut tris = Vec::with_capacity(2 * (n - 1) * (n - 1));
            for y in 0..n - 1 {
                for x in 0..n - 1 {
                    let a = grid[y * n + x];
                    let b = grid[y * n + x + 1];
                    let cc = grid[(y + 1) * n + x + 1];
                    let d = grid[(y + 1) * n + x];
                    // Alternate the split diagonal in a checkerboard, as displacements are tessellated.
                    if (x + y) % 2 == 0 {
                        tris.push([a, d, cc]);
                        tris.push([a, cc, b]);
                    } else {
                        tris.push([a, d, b]);
                        tris.push([b, d, cc]);
                    }
                }
            }
            for t in tris.iter_mut() {
                if t[1].sub(t[0]).cross(t[2].sub(t[0])).dot(up) < 0.0 {
                    t.swap(1, 2);
                }
            }
            let mut mins = grid[0];
            let mut maxs = grid[0];
            for p in &grid {
                for k in 0..3 {
                    mins.set(k, mins.get(k).min(p.get(k)));
                    maxs.set(k, maxs.get(k).max(p.get(k)));
                }
            }
            out.push(DispSurface { contents, mins, maxs, tris });
        }
        Ok(out)
    }
}

/// The swept box's half-width along `n`.
fn support(n: Vec3, extents: Vec3) -> f32 {
    n.x.abs() * extents.x + n.y.abs() * extents.y + n.z.abs() * extents.z
}

impl BspWorld {
    /// Brush indices under a model's head node.
    fn model_brushes(&self, head: i32) -> Vec<usize> {
        let mut seen = vec![false; self.brushes.len()];
        let mut out = Vec::new();
        let mut stack = vec![head];
        while let Some(n) = stack.pop() {
            if n < 0 {
                let Some(leaf) = self.leafs.get((-1 - n) as usize) else { continue };
                for &b in self.leaf_brushes.get(leaf.first_brush..leaf.first_brush + leaf.num_brushes).unwrap_or(&[]) {
                    let b = b as usize;
                    if b < seen.len() && !seen[b] {
                        seen[b] = true;
                        out.push(b);
                    }
                }
            } else if let Some(node) = self.nodes.get(n as usize) {
                stack.extend(node.children.iter().rev());
            }
        }
        out
    }

    /// Visit the leaves the swept box can touch, near side first, as the engine's tree walk does
    /// (without its fraction-based early out, so it may test a few extra brushes).
    #[allow(clippy::too_many_arguments)]
    fn walk(&self, n: i32, ray: &Ray, end: Vec3, mask: u32, seen: &mut [u64], tr: &mut TraceResult, hit_box: &mut bool) {
        if tr.all_solid {
            return;
        }
        if n < 0 {
            let Some(leaf) = self.leafs.get((-1 - n) as usize) else { return };
            for &b in self.leaf_brushes.get(leaf.first_brush..leaf.first_brush + leaf.num_brushes).unwrap_or(&[]) {
                let b = b as usize;
                if seen[b / 64] & (1 << (b % 64)) != 0 {
                    continue;
                }
                seen[b / 64] |= 1 << (b % 64);
                if self.contents[b] & mask == 0 {
                    continue;
                }
                let brush = &self.brushes[b];
                let before = tr.fraction;
                brush.clip_box(ray.start, ray.delta, ray.extents, tr);
                if tr.fraction < before {
                    *hit_box = brush.is_box();
                }
                if tr.all_solid {
                    return;
                }
            }
            return;
        }
        let Some(node) = self.nodes.get(n as usize) else { return };
        let p = self.planes[node.plane];
        let t1 = ray.start.dot(p.normal) - p.dist;
        let t2 = end.dot(p.normal) - p.dist;
        // One unit of slack keeps contacts within the 1/32 gap on both sides.
        let offset = support(p.normal, ray.extents) + 1.0;
        if t1 >= offset && t2 >= offset {
            self.walk(node.children[0], ray, end, mask, seen, tr, hit_box);
        } else if t1 < -offset && t2 < -offset {
            self.walk(node.children[1], ray, end, mask, seen, tr, hit_box);
        } else {
            let near = if t1 < t2 { 1 } else { 0 };
            self.walk(node.children[near], ray, end, mask, seen, tr, hit_box);
            self.walk(node.children[1 - near], ray, end, mask, seen, tr, hit_box);
        }
    }

    fn trace(&self, start: Vec3, end: Vec3, hull: Hull, mask: u32) -> TraceResult {
        let ray = Ray::new(start, end, hull);
        let mut tr = TraceResult::empty(end);
        let mut hit_box = false;
        let mut seen = vec![0u64; self.brushes.len().div_ceil(64)];
        let ray_end = ray.start.add(ray.delta);
        self.walk(self.world_head, &ray, ray_end, mask, &mut seen, &mut tr, &mut hit_box);

        let (lo, hi) = ray.bounds(start, end, hull);
        for (b, c) in &self.entity_brushes {
            if tr.all_solid {
                break;
            }
            if c & mask == 0 || !b.may_touch(lo, hi) {
                continue;
            }
            let before = tr.fraction;
            b.clip_box(ray.start, ray.delta, ray.extents, &mut tr);
            if tr.fraction < before {
                hit_box = b.is_box();
            }
        }
        if mask & MASK_PLAYERSOLID != 0 {
            for d in &self.disps {
                if tr.all_solid {
                    break;
                }
                let overlaps = (0..3).all(|k| lo.get(k) <= d.maxs.get(k) && hi.get(k) >= d.mins.get(k));
                if !overlaps {
                    continue;
                }
                for t in &d.tris {
                    let before = tr.fraction;
                    clip_box_to_triangle(t, &ray, lo, hi, &mut tr);
                    if tr.fraction < before {
                        hit_box = false;
                    }
                    if tr.all_solid {
                        break;
                    }
                }
            }
        }
        ray.finish(&mut tr, hit_box);
        tr
    }
}

/// Swept box against one displacement triangle, as a zero-thickness convex solid: the triangle's two
/// faces, its edge planes, and both directions of every separating axis a box and a triangle can have
/// (the box axes and each edge crossed with each box axis), so the Minkowski expansion is exact.
fn clip_box_to_triangle(t: &[Vec3; 3], ray: &Ray, lo: Vec3, hi: Vec3, tr: &mut TraceResult) {
    for k in 0..3 {
        let tmin = t[0].get(k).min(t[1].get(k)).min(t[2].get(k));
        let tmax = t[0].get(k).max(t[1].get(k)).max(t[2].get(k));
        if tmax < lo.get(k) || tmin > hi.get(k) {
            return;
        }
    }
    let mut n = t[1].sub(t[0]).cross(t[2].sub(t[0]));
    if n.normalize_in_place() < 1e-6 {
        return;
    }
    let mut planes: Vec<Plane> = Vec::with_capacity(29);
    let push_axis = |planes: &mut Vec<Plane>, mut a: Vec3, both: bool| {
        if a.normalize_in_place() < 1e-6 {
            return;
        }
        let hi = t[0].dot(a).max(t[1].dot(a)).max(t[2].dot(a));
        planes.push(Plane { normal: a, dist: hi });
        if both {
            let lo = t[0].dot(a).min(t[1].dot(a)).min(t[2].dot(a));
            planes.push(Plane { normal: a.neg(), dist: -lo });
        }
    };
    push_axis(&mut planes, n, true);
    let edges = [t[1].sub(t[0]), t[2].sub(t[1]), t[0].sub(t[2])];
    for e in edges {
        // Edge plane, facing away from the triangle.
        push_axis(&mut planes, e.cross(n), false);
    }
    for k in 0..3 {
        let mut axis = Vec3::ZERO;
        axis.set(k, 1.0);
        push_axis(&mut planes, axis, true);
        for e in edges {
            push_axis(&mut planes, e.cross(axis), true);
        }
    }
    clip_box_to_planes(&planes, EntityId::WORLD, ray.start, ray.delta, ray.extents, tr);
}

impl TraceWorld for BspWorld {
    fn trace_hull(&self, start: Vec3, end: Vec3, hull: Hull) -> TraceResult {
        self.trace(start, end, hull, MASK_PLAYERSOLID)
    }

    fn trace_ladder(&self, start: Vec3, end: Vec3, hull: Hull) -> TraceResult {
        self.trace(start, end, hull, CONTENTS_LADDER)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entities_parse_quoted_pairs() {
        let e = parse_entities(b"{\n\"classname\" \"worldspawn\"\n}\n{\n\"origin\" \"1 2 3\"\n\"angles\" \"0 90 0\"\n\"model\" \"*4\"\n}\n");
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].classname(), "worldspawn");
        assert_eq!(e[1].origin(), Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(e[1].yaw(), 90.0);
        assert_eq!(e[1].brush_model(), Some(4));
    }

    #[test]
    fn rejects_non_bsp() {
        assert!(BspMap::parse(b"not a map").is_err());
    }

    #[test]
    fn triangle_stops_a_falling_box_dist_epsilon_short() {
        let t = [Vec3::new(-100.0, -100.0, 0.0), Vec3::new(100.0, -100.0, 0.0), Vec3::new(0.0, 100.0, 0.0)];
        let (start, end) = (Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.0, -10.0));
        let ray = Ray::new(start, end, Hull::STAND);
        let (lo, hi) = ray.bounds(start, end, Hull::STAND);
        let mut tr = TraceResult::empty(end);
        clip_box_to_triangle(&t, &ray, lo, hi, &mut tr);
        ray.finish(&mut tr, false);
        assert!(tr.fraction < 1.0);
        assert!((tr.plane_normal.z - 1.0).abs() < 1e-6, "{:?}", tr.plane_normal);
        assert!((tr.end_pos.z - crate::trace::DIST_EPSILON).abs() < 1e-4, "{}", tr.end_pos.z);
    }

    #[test]
    fn triangle_edge_is_bevelled_for_the_box() {
        // A box beside the triangle's slanted edge but within its axial bounds must not hit.
        let t = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(100.0, 0.0, 0.0), Vec3::new(0.0, 100.0, 0.0)];
        let (start, end) = (Vec3::new(80.0, 80.0, 10.0), Vec3::new(80.0, 80.0, -10.0));
        let ray = Ray::new(start, end, Hull::STAND);
        let (lo, hi) = ray.bounds(start, end, Hull::STAND);
        let mut tr = TraceResult::empty(end);
        clip_box_to_triangle(&t, &ray, lo, hi, &mut tr);
        assert_eq!(tr.fraction, 1.0);
    }
}
