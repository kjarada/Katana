#include "katana/geometry/mesh.hpp"

#include <cmath>
#include <string>

#include "katana/geometry/point_batch.hpp"
#include "katana/geometry/polygon.hpp"
#include "katana/math/summation.hpp"

namespace katana::geometry {

double meshVolume(const TriangleMesh& mesh)
{
    if (mesh.vertices.empty()) {
        return 0.0;
    }
    const Point3 origin = mesh.vertices.front();
    katana::math::CompensatedSum volume;
    for (const auto& face : mesh.faces) {
        if (face[0] >= mesh.vertices.size() || face[1] >= mesh.vertices.size() ||
            face[2] >= mesh.vertices.size()) {
            continue;
        }
        const Point3 a = mesh.vertices[face[0]] - origin;
        const Point3 b = mesh.vertices[face[1]] - origin;
        const Point3 c = mesh.vertices[face[2]] - origin;
        volume.add(a.dot(b.cross(c)) / 6.0);
    }
    return volume.value();
}

using katana::core::ErrorCode;
using katana::core::makeError;
using katana::core::Status;

std::optional<Triangle3> TriangleMesh::triangle(std::size_t index) const
{
    if (index >= faces.size()) {
        return std::nullopt;
    }
    const auto& face = faces[index];
    const std::size_t count = vertices.size();
    if (face[0] >= count || face[1] >= count || face[2] >= count) {
        return std::nullopt;
    }
    return Triangle3{vertices[face[0]], vertices[face[1]], vertices[face[2]]};
}

katana::math::AABB TriangleMesh::bounds() const { return boundsOf(vertices); }

double TriangleMesh::area() const
{
    double total = 0.0;
    for (std::size_t i = 0; i < faces.size(); ++i) {
        if (const auto face = triangle(i)) {
            total += face->area();
        }
    }
    return total;
}

std::vector<Point2> TriangleMesh::planHull() const
{
    std::vector<Point2> plan;
    plan.reserve(vertices.size());
    for (const Point3& vertex : vertices) {
        plan.emplace_back(vertex.x, vertex.y);
    }
    return convexHull(std::move(plan));
}

Status validate(const TriangleMesh& mesh)
{
    for (std::size_t i = 0; i < mesh.vertices.size(); ++i) {
        const Point3& vertex = mesh.vertices[i];
        if (!std::isfinite(vertex.x) || !std::isfinite(vertex.y) || !std::isfinite(vertex.z)) {
            return makeError(ErrorCode::InvalidArgument, "a mesh vertex is not finite",
                             "vertex=" + std::to_string(i));
        }
    }
    for (std::size_t i = 0; i < mesh.faces.size(); ++i) {
        for (const std::uint32_t index : mesh.faces[i]) {
            if (index >= mesh.vertices.size()) {
                return makeError(ErrorCode::InvalidArgument,
                                 "a mesh face names a vertex that does not exist",
                                 "face=" + std::to_string(i) + " vertex=" + std::to_string(index));
            }
        }
    }
    return {};
}

} // namespace katana::geometry
