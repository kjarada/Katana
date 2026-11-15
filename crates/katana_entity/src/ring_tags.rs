//! Rings that are holes in an area (docs/geoprocessing.md, "Bindings";
//! docs/volumes.md, "Inside a polygon").
//!
//! The drawing has no polygon with a hole: an area is one closed ring, and a
//! ring drawn inside another is an area of its own. A ring is a hole only when
//! it says so. IMPORT writes `source.ring` (`exterior` or `hole`) and
//! `source.part` (the file's feature) into an entity's metadata, and a
//! geoprocessing result writes `gis.ring` and `gis.part` into its properties.
//!
//! This is the one reading of those tags and the one rule that joins a hole to
//! the exterior it lies in, so a lot with a hole has the same area whichever
//! reads it. It lives in the entity layer because interop may not see cad and
//! cad may not see interop.

use katana_core::text::lowered;
use katana_geometry::{Point2, Polyline2};

use crate::entity::{Entity, EntityId, property_to_string};

/// A property or a metadata value, as text: IMPORT writes source.ring into an
/// entity's metadata, a result writes gis.ring into its properties.
fn tag_of(entity: &Entity, key: &str) -> Option<String> {
    entity
        .properties
        .get(key)
        .or_else(|| entity.metadata.get(key))
        .map(property_to_string)
}

/// True when `gis.ring` or `source.ring` says `hole`, in any case, in the
/// properties or the metadata.
pub fn is_tagged_hole(entity: &Entity) -> bool {
    let is_hole = |tag: Option<String>| tag.is_some_and(|tag| lowered(tag) == b"hole");
    is_hole(tag_of(entity, "gis.ring")) || is_hole(tag_of(entity, "source.ring"))
}

/// Which polygon a ring came from: `gis.part`, which a result writes (the
/// exterior's id), or else `source.part`, which IMPORT writes. None when the
/// entity says neither.
pub fn ring_part(entity: &Entity) -> Option<String> {
    tag_of(entity, "gis.part").or_else(|| tag_of(entity, "source.part"))
}

/// A ring a hole may join.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ExteriorCandidate {
    pub id: EntityId,
    pub part: Option<String>,
    /// Closed, and never repeating its first vertex at the end.
    pub ring: Polyline2,
}

/// Chooses, for a hole, the exterior it joins.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HoleJoiner {
    exteriors: Vec<ExteriorCandidate>,
    areas: Vec<f64>,
}

impl HoleJoiner {
    pub fn new(exteriors: Vec<ExteriorCandidate>) -> Self {
        let areas = exteriors
            .iter()
            .map(|exterior| exterior.ring.area())
            .collect();
        Self { exteriors, areas }
    }

    /// The index (in the exteriors given) of the exterior the hole joins, or
    /// None when no ring holds every vertex of it - the hole then stays an
    /// area of its own, and the caller says so. Of the rings that hold it, the
    /// one whose part the hole shares when there is such; else the smallest in
    /// area; and of equals the one made nearest before the hole (the greatest
    /// id below the hole's). A vertex on a ring's edge is held by it, as
    /// Polyline2::contains says.
    pub fn exterior_of(
        &self,
        hole: &[Point2],
        hole_id: EntityId,
        hole_part: &Option<String>,
    ) -> Option<usize> {
        // Made before the hole, and how long before: the nearer the better.
        let before = |id: EntityId| {
            if id < hole_id {
                hole_id - id
            } else {
                EntityId::MAX
            }
        };
        let mut best: Option<usize> = None;
        let mut best_shares_part = false;
        for (a, exterior) in self.exteriors.iter().enumerate() {
            let inside = hole.iter().all(|&point| exterior.ring.contains(point));
            if !inside {
                continue;
            }
            let shares_part = hole_part.is_some() && exterior.part == *hole_part;
            let better = match best {
                None => true,
                Some(current) => {
                    (shares_part && !best_shares_part)
                        || (shares_part == best_shares_part
                            && (self.areas[a] < self.areas[current]
                                || (self.areas[a] == self.areas[current]
                                    && before(exterior.id) < before(self.exteriors[current].id))))
                }
            };
            if better {
                best = Some(a);
                best_shares_part = shares_part;
            }
        }
        best
    }
}
