#pragma once

// Rings that are holes in an area (docs/geoprocessing.md, "Bindings";
// docs/volumes.md, "Inside a polygon").
//
// The drawing has no polygon with a hole: an area is one closed ring, and a
// ring drawn inside another is an area of its own (a building inside a lot is
// not a hole in the lot). A ring is a hole only when it says so. IMPORT writes
// `source.ring` (`exterior` or `hole`) and `source.part` (the file's feature)
// into an entity's metadata, and a geoprocessing result writes `gis.ring` and
// `gis.part` into its properties.
//
// This is the one reading of those tags and the one rule that joins a hole to
// the exterior it lies in. The feature-table conversion
// (interop::drawingDataset) and the volume verbs both use it, so a lot with a
// hole has the same area whichever reads it. It lives in the entity layer
// because interop may not see cad and cad may not see interop.

#include <cstddef>
#include <optional>
#include <string>
#include <vector>

#include "katana/entity/entity.hpp"
#include "katana/geometry/primitives2d.hpp"

namespace katana::entity {

// True when `gis.ring` or `source.ring` says `hole`, in any case, in the
// properties or the metadata.
[[nodiscard]] bool isTaggedHole(const Entity& entity);

// Which polygon a ring came from: `gis.part`, which a result writes (the
// exterior's id), or else `source.part`, which IMPORT writes. None when the
// entity says neither.
[[nodiscard]] std::optional<std::string> ringPart(const Entity& entity);

// A ring a hole may join.
struct ExteriorCandidate {
    EntityId id = 0;
    std::optional<std::string> part{};
    // Closed, and never repeating its first vertex at the end.
    katana::geometry::Polyline2 ring{};
};

// Chooses, for a hole, the exterior it joins.
class HoleJoiner {
  public:
    explicit HoleJoiner(std::vector<ExteriorCandidate> exteriors);

    // The index (in the exteriors given) of the exterior the hole joins, or
    // none when no ring holds every vertex of it - the hole then stays an area
    // of its own, and the caller says so. Of the rings that hold it, the one
    // whose part the hole shares when there is such; else the smallest in area;
    // and of equals the one made nearest before the hole (the greatest id below
    // the hole's) - the same file imported twice gives two exteriors equal in
    // everything but their ids, and each hole was made just after its own.
    // A vertex on a ring's edge is held by it, as Polyline2::contains says.
    [[nodiscard]] std::optional<std::size_t>
    exteriorOf(const std::vector<katana::geometry::Point2>& hole, EntityId holeId,
               const std::optional<std::string>& holePart) const;

  private:
    std::vector<ExteriorCandidate> exteriors_;
    std::vector<double> areas_;
};

} // namespace katana::entity
