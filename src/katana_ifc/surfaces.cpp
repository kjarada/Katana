// Surfaces -> IfcGeographicElement TERRAIN.
//
// A TIN is written as an IfcTriangulatedFaceSet (Body, Tessellation): its
// points once, its triangles as one-based triples of them. IFC 4.3 also has
// IfcTriangulatedIrregularNetwork, a face set that can flag breaklines, but
// the Alignment-based view this file declares does not include it (the
// validation service's IFC431 lists it as out of that view's scope), and a
// face set is what every viewer draws.

#include <string>

#include "katana/terrain/tin_surface.hpp"
#include "parts.hpp"

namespace katana::ifc::detail {

void exportSurfaces(Builder& builder, const std::vector<SurfaceInput>& surfaces)
{
    for (const SurfaceInput& input : surfaces) {
        const terrain::TinSurface* surface = input.surface;
        if (surface == nullptr || surface->triangleCount() == 0) {
            builder.warn("surface \"" + input.name + "\" has no triangles and is not written");
            continue;
        }
        std::vector<std::string> coordinates;
        coordinates.reserve(surface->vertexCount());
        for (const auto& vertex : surface->vertices()) {
            const Vec3 at = builder.local(Vec3(vertex.x, vertex.y, vertex.z));
            coordinates.push_back(listOf({stepReal(at.x), stepReal(at.y), stepReal(at.z)}));
        }
        std::vector<std::string> triangles;
        triangles.reserve(surface->triangleCount());
        for (const auto& triangle : surface->triangles()) {
            triangles.push_back(
                listOf({std::to_string(triangle[0] + 1), std::to_string(triangle[1] + 1),
                        std::to_string(triangle[2] + 1)}));
        }
        StepFile& file = builder.file();
        const Id points =
            file.add("IfcCartesianPointList3D", Args().raw(listOf(coordinates)).null());
        const Id faces =
            file.add("IfcTriangulatedFaceSet",
                     Args().ref(points).null().boolean(false).raw(listOf(triangles)).null());
        const Id representation =
            builder.shape(builder.bodyContext(), "Body", "Tessellation", {faces});
        builder.layer("Surfaces/" + input.name, representation);
        const std::string key = "surface/" + input.name;
        const Id element = builder.product({"IfcGeographicElement", "TERRAIN", {}}, key, input.name,
                                           "Surface", builder.productShape({representation}));
        builder.tally("surface " + input.name, {"IfcGeographicElement", "TERRAIN", {}}, {},
                      "a surface: its triangles");
        PropertyList list;
        list.integer("Points", static_cast<long long>(surface->vertexCount()));
        list.integer("Triangles", static_cast<long long>(surface->triangleCount()));
        builder.defines(key, builder.propertySet(key, "Katana_Surface", list), {element});
        ++builder.report().surfaces;
    }
}

} // namespace katana::ifc::detail
