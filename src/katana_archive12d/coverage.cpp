#include "katana/archive12d/coverage.hpp"

namespace katana::archive12d {

const std::vector<ElementCoverage>& elementCoverage()
{
    // In the manual's order. "becomes" is the sentence docs/interop.md prints.
    static const std::vector<ElementCoverage> rows = {
        {"model", "1.4.1", Handling::Full,
         "a layer; a tree name such as Stage 1/Water arrives as that nested layer"},
        {"colour", "1.4.2", Handling::Full,
         "the entity's colour where the name is one of 12d's standard colours; the name is "
         "always kept"},
        {"style", "1.4.3", Handling::Full, "kept on the entity as 12d.style and written back"},
        {"breakline", "1.4.4", Handling::Full,
         "kept on the entity as 12d.breakline and written back"},
        {"null", "1.4.5", Handling::Full,
         "a height equal to the null value, or the null keyword, is no height at all"},
        {"attributes", "1.3", Handling::Full,
         "typed entity properties; a group flattens into Group/Name and is rebuilt on export"},
        {"project_attributes", "-", Handling::ReadOnly,
         "read into the archive; a Katana project has no attributes of its own"},
        {"tin", "1.4.7.2", Handling::Full, "a surface (terrain::TinSurface)"},
        {"full_tin", "1.4.7.1", Handling::ImportOnly,
         "a surface of its visible, non-construction triangles; written back as a tin"},
        {"super_tin", "1.4.8", Handling::ReadOnly,
         "reported; its member tins are what is imported"},
        {"primitive_3d", "1.4.9", Handling::Full,
         "a mesh in the session (geometry::TriangleMesh), drawn in 3D and as its footprint in "
         "plan; written back with its per-face colours"},
        {"string arc", "1.5.1", Handling::ImportOnly,
         "an Arc; exported arcs are written as two-vertex super strings"},
        {"string circle", "1.5.2", Handling::Full, "a Circle"},
        {"string drainage", "1.5.3", Handling::ImportOnly,
         "the line as a Polyline carrying its pipes, each pit and house connection as a Point"},
        {"string face", "1.5.4", Handling::ImportOnly, "a closed or open Polyline"},
        {"string feature", "1.5.5", Handling::ImportOnly, "a Circle"},
        {"string interface", "1.5.6", Handling::ImportOnly, "a Polyline"},
        {"string plot_frame", "1.5.7", Handling::ImportOnly,
         "the sheet rectangle, paper size times plot scale, as a closed Polyline"},
        {"string super", "1.5.8", Handling::Full,
         "a Point, Line or Polyline; arcs and transitions chorded to a stated tolerance"},
        {"string super_alignment", "1.5.9", Handling::Full,
         "a named Alignment where its geometry is PI-definable and verifies, and always a "
         "Polyline of the centreline"},
        {"string text", "1.5.10", Handling::Full, "Text"},
        {"string 2d", "1.5.11", Handling::ImportOnly, "as string super"},
        {"string 3d", "1.5.12", Handling::ImportOnly, "as string super"},
        {"string 4d", "1.5.13", Handling::ImportOnly,
         "as string super, with its vertex text as Text"},
        {"string pipe", "1.5.14", Handling::ImportOnly, "as string super, with its diameter"},
        {"string polyline", "1.5.15", Handling::ImportOnly, "as string super"},
        {"string alignment", "1.5.16", Handling::ImportOnly, "as string super_alignment"},
        {"string pipeline", "1.5.17", Handling::ImportOnly, "as string super_alignment"},
        {"string las_cloud_data", "1.5.18", Handling::ImportOnly,
         "a point cloud reference layer; all eleven point record formats, tagged and compact"},
    };
    return rows;
}

std::string_view toString(Handling handling)
{
    switch (handling) {
    case Handling::Full:
        return "import and export";
    case Handling::ImportOnly:
        return "import";
    case Handling::ReadOnly:
        return "read";
    }
    return "read";
}

} // namespace katana::archive12d
