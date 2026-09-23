#pragma once

// The tool families: each is one file in this directory that adds its tools to
// the catalogue (include/katana/cad/interactive_tool.hpp). The list is explicit
// rather than self-registering on purpose: katana_cad is a static archive, and
// an object file that nothing references is dropped by the linker, taking its
// tools with it silently (the trap src/katana_surveyio/CMakeLists.txt measured).
// A call from toolCatalog() references every family, so none can go missing.
//
// A family adds each tool with `report(catalog.add(info))`, so a refused tool
// is recorded rather than lost.

#include <functional>

#include "katana/cad/interactive_tool.hpp"

namespace katana::cad::tools {

using Report = std::function<void(katana::core::Status)>;

// Point, Line, Polyline, Rectangle, Polygon.
void addDrawLineTools(ToolCatalog& catalog, const Report& report);
// Circle and Arc in their construction variants.
void addDrawCurveTools(ToolCatalog& catalog, const Report& report);
// Move, Copy, Rotate, Scale, Mirror, Array, Erase.
void addModifyTransformTools(ToolCatalog& catalog, const Report& report);
// Trim, Extend, Offset, Fillet, Chamfer, Break, Join, Explode.
void addModifyEditTools(ToolCatalog& catalog, const Report& report);
// Text and dimensions.
void addAnnotateTools(ToolCatalog& catalog, const Report& report);
// Distance, Area, ID Point, Angle, List - tools that measure and report.
void addInquiryTools(ToolCatalog& catalog, const Report& report);

} // namespace katana::cad::tools
