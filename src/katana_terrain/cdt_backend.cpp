#include "cdt_backend.hpp"

// The only translation unit of Katana that includes CGAL (PLAN.MD Rule 4).
//
// Kernel: Exact_predicates_inexact_constructions_kernel. Orientation and
// in-circle tests are evaluated exactly (filtered: interval arithmetic first,
// exact rationals when the interval is inconclusive), so the topology of the
// triangulation is always that of the true Delaunay triangulation of the given
// doubles; no tolerance is involved. The only constructed coordinates are the
// crossing points of constraints (Exact_predicates_tag): CGAL computes them in
// double precision and, when the rounded point falls outside the two triangles
// it must lie in, recomputes them with exact rationals and rounds once.
//
// Constrained_triangulation_plus_2 keeps the hierarchy from input constraints to
// the sub-edges they were split into. That gives (a) the vertices along each
// constraint, needed to interpolate breakline elevations at crossings, and (b)
// crossings of later constraints computed against the original segment rather
// than against an already rounded piece.

#include <algorithm>
#include <cstddef>
#include <exception>
#include <new>
#include <numeric>
#include <string>
#include <utility>

#include <CGAL/Constrained_Delaunay_triangulation_2.h>
#include <CGAL/Constrained_triangulation_face_base_2.h>
#include <CGAL/Constrained_triangulation_plus_2.h>
#include <CGAL/Exact_predicates_inexact_constructions_kernel.h>
#include <CGAL/Spatial_sort_traits_adapter_2.h>
#include <CGAL/Triangulation_face_base_with_info_2.h>
#include <CGAL/Triangulation_vertex_base_with_info_2.h>
#include <CGAL/property_map.h>
#include <CGAL/spatial_sort.h>

namespace katana::terrain::detail {

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Result;
using katana::geometry::Point2;

namespace {

// Built-in info types would be left uninitialised by CGAL for the vertices it
// creates itself; the default member initialiser is what marks those vertices.
struct VertexInfo {
    std::uint32_t index = kNoIndex;
};
struct FaceInfo {
    std::uint32_t index = kNoIndex;
};

using Kernel = CGAL::Exact_predicates_inexact_constructions_kernel;
using VertexBase = CGAL::Triangulation_vertex_base_with_info_2<VertexInfo, Kernel>;
using FaceInfoBase = CGAL::Triangulation_face_base_with_info_2<FaceInfo, Kernel>;
using FaceBase = CGAL::Constrained_triangulation_face_base_2<Kernel, FaceInfoBase>;
using DataStructure = CGAL::Triangulation_data_structure_2<VertexBase, FaceBase>;
using BaseTriangulation =
    CGAL::Constrained_Delaunay_triangulation_2<Kernel, DataStructure, CGAL::Exact_predicates_tag>;
using Triangulation = CGAL::Constrained_triangulation_plus_2<BaseTriangulation>;
using VertexHandle = Triangulation::Vertex_handle;
using FaceHandle = Triangulation::Face_handle;
using ConstraintId = Triangulation::Constraint_id;

struct GeneratedVertex {
    Point2 position;
    VertexHandle handle;
};

// A triangle in canonical rotation plus what is needed to find it again.
struct FaceRecord {
    std::array<std::uint32_t, 3> vertices{};
    FaceHandle handle;
    std::uint8_t rotation = 0; // canonical vertex j is CGAL vertex (j + rotation) % 3
};

// Katana edge k joins vertices k and k + 1; CGAL names an edge by the vertex
// opposite to it, which for that edge is vertex k + 2.
int cgalEdgeIndex(std::size_t canonicalEdge, std::uint8_t rotation)
{
    return static_cast<int>((canonicalEdge + rotation + 2) % 3);
}

Result<CdtOutput> triangulateUnchecked(const CdtInput& input)
{
    const std::size_t pointCount = input.points.size();

    // Insertion order: CGAL's spatial sort (Hilbert curve with a fixed-seed
    // shuffle) makes point location during insertion O(1) on average. The order
    // is a pure function of the coordinates, hence deterministic.
    std::vector<Kernel::Point_2> points;
    points.reserve(pointCount);
    for (const Point2& p : input.points) {
        points.emplace_back(p.x, p.y);
    }
    std::vector<std::size_t> order(pointCount);
    std::iota(order.begin(), order.end(), std::size_t{0});
    using SortTraits =
        CGAL::Spatial_sort_traits_adapter_2<Kernel,
                                            CGAL::Pointer_property_map<Kernel::Point_2>::const_type>;
    CGAL::spatial_sort(order.begin(), order.end(),
                       SortTraits(CGAL::make_property_map(std::as_const(points)), Kernel()));

    Triangulation triangulation;
    std::vector<VertexHandle> handles(pointCount);
    FaceHandle hint;
    for (const std::size_t index : order) {
        const VertexHandle vertex = triangulation.insert(points[index], hint);
        if (vertex->info().index != kNoIndex) {
            return makeError(ErrorCode::InvalidArgument,
                             "triangulation points must be pairwise distinct",
                             "points " + std::to_string(vertex->info().index) + " and " +
                                 std::to_string(index));
        }
        vertex->info().index = static_cast<std::uint32_t>(index);
        handles[index] = vertex;
        hint = vertex->face();
    }
    if (triangulation.dimension() < 2) {
        return makeError(ErrorCode::TriangulationFailure,
                         "all positions are collinear; no triangle can be formed",
                         "distinct positions=" + std::to_string(pointCount));
    }

    // Constraints, in input order. CGAL numbers its constraint ids sequentially;
    // tagOfId maps such an id back to the caller's tag.
    std::vector<ConstraintId> constraintIds(input.constraints.size());
    std::vector<std::uint8_t> tagOfId;
    for (std::size_t c = 0; c < input.constraints.size(); ++c) {
        const CdtConstraint& constraint = input.constraints[c];
        if (constraint.from == constraint.to) {
            continue;
        }
        const ConstraintId id =
            triangulation.insert_constraint(handles[constraint.from], handles[constraint.to]);
        if (id == nullptr) {
            continue;
        }
        constraintIds[c] = id;
        if (tagOfId.size() <= id.index()) {
            tagOfId.resize(id.index() + 1, std::uint8_t{0});
        }
        tagOfId[id.index()] = constraint.tag;
    }

    if (triangulation.number_of_vertices() >= kNoIndex ||
        triangulation.number_of_faces() >= kNoIndex) {
        return makeError(ErrorCode::InvalidArgument,
                         "triangulation exceeds the 32-bit index range",
                         "vertices=" + std::to_string(triangulation.number_of_vertices()));
    }

    CdtOutput output;

    // Vertices created at constraint crossings, numbered in (x, y) order so the
    // numbering does not depend on CGAL's internal storage order.
    std::vector<GeneratedVertex> generated;
    for (auto vertex = triangulation.finite_vertices_begin();
         vertex != triangulation.finite_vertices_end(); ++vertex) {
        if (vertex->info().index == kNoIndex) {
            generated.push_back({Point2(vertex->point().x(), vertex->point().y()), vertex});
        }
    }
    std::sort(generated.begin(), generated.end(),
              [](const GeneratedVertex& a, const GeneratedVertex& b) {
                  return a.position.x < b.position.x ||
                         (a.position.x == b.position.x && a.position.y < b.position.y);
              });
    output.generatedPoints.reserve(generated.size());
    for (std::size_t i = 0; i < generated.size(); ++i) {
        generated[i].handle->info().index = static_cast<std::uint32_t>(pointCount + i);
        output.generatedPoints.push_back(generated[i].position);
    }

    // Triangles in canonical rotation, then sorted.
    std::vector<FaceRecord> faces;
    faces.reserve(triangulation.number_of_faces());
    for (auto face = triangulation.finite_faces_begin(); face != triangulation.finite_faces_end();
         ++face) {
        const std::array<std::uint32_t, 3> v{face->vertex(0)->info().index,
                                             face->vertex(1)->info().index,
                                             face->vertex(2)->info().index};
        const auto smallest = static_cast<std::uint8_t>(std::min_element(v.begin(), v.end()) -
                                                        v.begin());
        FaceRecord record;
        record.handle = face;
        record.rotation = smallest;
        for (std::size_t j = 0; j < 3; ++j) {
            record.vertices[j] = v[(j + smallest) % 3];
        }
        faces.push_back(record);
    }
    std::sort(faces.begin(), faces.end(), [](const FaceRecord& a, const FaceRecord& b) {
        return a.vertices < b.vertices;
    });
    for (std::size_t t = 0; t < faces.size(); ++t) {
        faces[t].handle->info().index = static_cast<std::uint32_t>(t);
    }

    const std::size_t triangleCount = faces.size();
    output.triangles.resize(triangleCount);
    output.neighbors.resize(triangleCount);
    output.edgeTags.assign(triangleCount, std::array<std::uint8_t, 3>{0, 0, 0});
    for (std::size_t t = 0; t < triangleCount; ++t) {
        const FaceRecord& record = faces[t];
        output.triangles[t] = record.vertices;
        for (std::size_t k = 0; k < 3; ++k) {
            const int edge = cgalEdgeIndex(k, record.rotation);
            const FaceHandle neighbor = record.handle->neighbor(edge);
            output.neighbors[t][k] =
                triangulation.is_infinite(neighbor) ? kNoIndex : neighbor->info().index;

            if (!record.handle->is_constrained(edge)) {
                continue;
            }
            std::uint8_t tags = 0;
            const VertexHandle a = record.handle->vertex(Triangulation::ccw(edge));
            const VertexHandle b = record.handle->vertex(Triangulation::cw(edge));
            for (const auto& context : triangulation.contexts(a, b)) {
                const std::size_t id = context.id().index();
                tags = static_cast<std::uint8_t>(tags | (id < tagOfId.size() ? tagOfId[id] : 0));
            }
            output.edgeTags[t][k] = tags;
        }
    }

    if (input.exportConstraintVertices) {
        output.constraintVertexOffsets.reserve(input.constraints.size() + 1);
        output.constraintVertexOffsets.push_back(0);
        for (std::size_t c = 0; c < input.constraints.size(); ++c) {
            if (constraintIds[c] != nullptr) {
                const std::size_t first = output.constraintVertices.size();
                for (const VertexHandle vertex :
                     triangulation.vertices_in_constraint(constraintIds[c])) {
                    output.constraintVertices.push_back(vertex->info().index);
                }
                // Documented direction is from -> to, whatever the kernel stores.
                if (output.constraintVertices[first] != input.constraints[c].from) {
                    std::reverse(output.constraintVertices.begin() +
                                     static_cast<std::ptrdiff_t>(first),
                                 output.constraintVertices.end());
                }
            }
            output.constraintVertexOffsets.push_back(
                static_cast<std::uint32_t>(output.constraintVertices.size()));
        }
    }
    return output;
}

} // namespace

Result<CdtOutput> triangulate(const CdtInput& input)
{
    if (input.points.size() < 3) {
        return makeError(ErrorCode::TriangulationFailure,
                         "a triangulation needs at least 3 distinct positions",
                         "distinct positions=" + std::to_string(input.points.size()));
    }
    if (input.points.size() >= kNoIndex - 1) {
        return makeError(ErrorCode::InvalidArgument, "too many points for 32-bit indices",
                         "points=" + std::to_string(input.points.size()));
    }
    for (std::size_t c = 0; c < input.constraints.size(); ++c) {
        const CdtConstraint& constraint = input.constraints[c];
        if (constraint.from >= input.points.size() || constraint.to >= input.points.size()) {
            return makeError(ErrorCode::InvalidArgument, "constraint names a point out of range",
                             "constraint=" + std::to_string(c));
        }
        if (constraint.tag == 0) {
            return makeError(ErrorCode::InvalidArgument, "constraint tag must be non-zero",
                             "constraint=" + std::to_string(c));
        }
    }

    // CGAL reports violated internal checks by throwing (its default failure
    // behaviour). Those are expected-failure paths for Katana, not programming
    // errors of the caller, so they become Result errors here. Running out of
    // memory is not a triangulation failure and keeps propagating.
    try {
        return triangulateUnchecked(input);
    } catch (const std::bad_alloc&) {
        throw;
    } catch (const std::exception& exception) {
        return makeError(ErrorCode::TriangulationFailure, "triangulation kernel failed",
                         exception.what());
    } catch (...) {
        return makeError(ErrorCode::TriangulationFailure, "triangulation kernel failed",
                         "unknown exception");
    }
}

} // namespace katana::terrain::detail
