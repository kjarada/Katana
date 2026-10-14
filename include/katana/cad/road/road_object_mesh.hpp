#pragma once

// The solids a road object is made of (docs/road_objects.md, "How an object
// is made"): a polygon extruded, a cross-section swept along a path, a tube.
// Every function here ADDS a closed, outward-oriented shell to a mesh - each
// edge used by exactly two triangles, once in each direction - so the 3D
// view can cull back faces, the IFC export can write the mesh as a closed
// face set, and a volume is the divergence theorem over the triangles.
//
// A cross-section is given in (u, v): u across, positive to the LEFT of the
// direction it is swept in, v up. It stays VERTICAL wherever it is swept -
// a barrier or a wall stands upright on a grade, it does not lean with it -
// and is mitred at a bend of the path, so a constant section keeps its width
// round a corner.

#include <array>
#include <cstdint>
#include <span>
#include <vector>

#include "katana/geometry/mesh.hpp"
#include "katana/geometry/primitives2d.hpp"
#include "katana/geometry/primitives3d.hpp"

namespace katana::cad::road {

// A point of a path a section is swept along: where it is in plan and the
// level the section's v = 0 stands at there.
struct PathPoint {
    katana::geometry::Point2 position;
    double z = 0.0;

    friend bool operator==(const PathPoint&, const PathPoint&) = default;
};

// The triangles that fill a simple polygon, as index triples into it, each
// counter-clockwise: ear clipping, which is quadratic and meant for the few
// vertices of a cross-section or a sign's plate. The polygon may be concave
// (an F-shape barrier's knee) and either way round;
// collinear vertices are fine. Empty for fewer than three vertices, or for a
// polygon no ear can be cut from (one that crosses itself).
[[nodiscard]] std::vector<std::array<std::uint32_t, 3>>
triangulatePolygon(std::span<const katana::geometry::Point2> polygon);

// The polygon counter-clockwise: as given, or reversed when it was clockwise.
[[nodiscard]] std::vector<katana::geometry::Point2>
counterClockwise(std::span<const katana::geometry::Point2> polygon);

// A regular polygon of `sides` vertices on a circle of `radius` about
// `centre`, counter-clockwise, the first at `startRadians`. The round things
// - posts, a pole, a pipe - are drawn so: a polygon of enough sides reads as
// round, and its volume is known exactly.
[[nodiscard]] std::vector<katana::geometry::Point2>
regularPolygon(katana::geometry::Point2 centre, double radius, int sides,
               double startRadians = 0.0);

// `polygon`, in the plane through `origin` spanned by the unit vectors `u`
// and `v`, extruded `depth` along u x v. `u` and `v` must be perpendicular
// unit vectors; the polygon may be either way round. Adds nothing for a
// polygon that does not triangulate or a depth that is not positive.
void addExtrusion(katana::geometry::TriangleMesh& mesh,
                  std::span<const katana::geometry::Point2> polygon,
                  const katana::geometry::Point3& origin, const katana::geometry::Vec3& u,
                  const katana::geometry::Vec3& v, double depth);

// A vertical prism: `outline` in plan, from level `bottom` up to `top`.
void addPrism(katana::geometry::TriangleMesh& mesh,
              std::span<const katana::geometry::Point2> outline, double bottom, double top);

// A box standing on `baseCentre`: `length` along the horizontal unit vector
// `along`, `width` across it, `height` up.
void addBox(katana::geometry::TriangleMesh& mesh, const katana::geometry::Point3& baseCentre,
            const katana::geometry::Vec2& along, double length, double width, double height);

// `profile`, in (u, v), swept along `path`: a closed solid with a flat end at
// each end of the path. Points of the path closer than a millimetre to the
// one before are passed over. Adds nothing for fewer than two distinct
// points or a profile that does not triangulate.
void addSweep(katana::geometry::TriangleMesh& mesh, std::span<const PathPoint> path,
              std::span<const katana::geometry::Point2> profile);

// A hollow sweep - a pipe, a box culvert's barrel: the solid between
// `outer` and `inner`, which have the same number of vertices, correspond
// vertex by vertex, and are both counter-clockwise with `inner` inside
// `outer`. Adds nothing when they differ in size or there are fewer than two
// distinct points.
void addHollowSweep(katana::geometry::TriangleMesh& mesh, std::span<const PathPoint> path,
                    std::span<const katana::geometry::Point2> outer,
                    std::span<const katana::geometry::Point2> inner);

// A closed mesh's volume is geometry::meshVolume (geometry/mesh.hpp).

// True when every edge of every face is used by exactly one other face in
// the opposite direction: a closed, consistently oriented surface - what a
// solid needs to be. False for an empty mesh.
[[nodiscard]] bool isClosedMesh(const katana::geometry::TriangleMesh& mesh);

} // namespace katana::cad::road
