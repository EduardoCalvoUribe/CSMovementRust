//! Trigger volumes. A zone is a set of brushes; overlap uses the same hull-versus-brush test the movement
//! code uses for stuck checks, so no new collision code exists here.

use movement::trace::TraceWorld;
use movement::world::{Brush, PrimitiveWorld};
use movement::{Hull, Vec3};

#[derive(Clone, Debug, Default)]
pub struct Zone {
    world: PrimitiveWorld,
}

impl Zone {
    pub fn from_brushes(brushes: impl IntoIterator<Item = Brush>) -> Self {
        let mut world = PrimitiveWorld::new();
        for b in brushes {
            world.add(b);
        }
        Self { world }
    }

    /// An axis-aligned box zone.
    pub fn cuboid(min: Vec3, max: Vec3) -> Self {
        Self::from_brushes([Brush::cuboid(min, max)])
    }

    /// Axis-aligned bounds of all brushes, or `None` for an empty zone.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut it = self.world.brushes.iter();
        let first = it.next()?;
        let (mut lo, mut hi) = (first.mins, first.maxs);
        for b in it {
            lo = Vec3::new(lo.x.min(b.mins.x), lo.y.min(b.mins.y), lo.z.min(b.mins.z));
            hi = Vec3::new(hi.x.max(b.maxs.x), hi.y.max(b.maxs.y), hi.z.max(b.maxs.z));
        }
        Some((lo, hi))
    }

    /// Center of [`Zone::bounds`]. Used as the goal point for observations, not by any run rule.
    pub fn center(&self) -> Option<Vec3> {
        self.bounds().map(|(lo, hi)| Vec3::new((lo.x + hi.x) * 0.5, (lo.y + hi.y) * 0.5, (lo.z + hi.z) * 0.5))
    }

    /// The hull standing at `origin` overlaps the zone. Exact contact counts as overlap, as in brush
    /// tracing.
    pub fn overlaps(&self, origin: Vec3, hull: Hull) -> bool {
        self.world.point_solid(origin, hull)
    }
}
