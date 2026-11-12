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

    /// The hull standing at `origin` overlaps the zone. Exact contact counts as overlap, as in brush
    /// tracing.
    pub fn overlaps(&self, origin: Vec3, hull: Hull) -> bool {
        self.world.point_solid(origin, hull)
    }
}
