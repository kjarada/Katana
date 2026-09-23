// Text and dimensions (see families.hpp). The tools themselves are in
// annotate_text.cpp, annotate_dimension.cpp and annotate_leader.cpp; this file
// lists them, so the one call toolCatalog() makes keeps every one of them in
// the link.
//
// Aliases are AutoCAD's: TEXT and DTEXT/DT for single-line text, DIMLINEAR/DLI,
// DIMALIGNED/DAL, and LEADER with both LEAD (AutoCAD's alias for LEADER) and
// LE (QLEADER's, the leader most users now reach for). T is left to the
// command interpreter, where it already means TEXT with its arguments typed on
// one line, and DIM likewise stays the interpreter's aligned "DIM p p offset".
//
// RADIUS, DIAMETER AND ANGULAR DIMENSIONS ARE NOT LISTED, because the entity
// model cannot carry them: its one dimension, DimensionGeometry, is an aligned
// dimension between two points. Drawing them as lines and a text would be a
// dimension in appearance only - a label that stays "R5" when the arc is
// scaled to R10, ignores the dimension style and is not selectable as a
// dimension - so they wait for the model to have them. What it would need:
//
//   * radius:   the arc's centre, a point on it where the dimension line
//               meets it, and a leader length or text position; measures the
//               radius, labelled with an "R" prefix;
//   * diameter: two diametrically opposite points on the circle (or the
//               centre and one), measuring their distance, labelled with a
//               diameter sign;
//   * angular:  a vertex and a point on each of the two arms (or the two
//               lines' four points), and the radius of the dimension arc;
//               measures the angle in the style's angular units and precision.
//
// Each is a new appended Geometry alternative (or a kind field and the extra
// points on DimensionGeometry), with its drawing in dimension_draw.cpp, its
// wire format in geometry_blob.cpp and the rest of docs/model.md's list.

#include "annotate_common.hpp"
#include "families.hpp"

namespace katana::cad::tools {

void addAnnotateTools(ToolCatalog& catalog, const Report& report)
{
    report(catalog.add(ToolInfo{
        .id = "annotate.text",
        .name = "Text",
        .category = "Annotate",
        .group = "Text",
        .order = 1,
        .aliases = {"TEXT", "DTEXT", "DT"},
        .shortcut = {},
        .tip = "Places single-line text: pick the start point, give the height and rotation, "
               "then type each line; an empty line finishes.",
        .make = annotate::makeTextTool,
    }));
    report(catalog.add(ToolInfo{
        .id = "annotate.dimlinear",
        .name = "Linear Dimension",
        .category = "Annotate",
        .group = "Dimensions",
        .order = 1,
        .aliases = {"DIMLINEAR", "DLI"},
        .shortcut = {},
        .tip = "Dimensions the horizontal or vertical distance between two points, chosen by "
               "where the dimension line is placed or by typing H or V.",
        .make = annotate::makeLinearDimensionTool,
    }));
    report(catalog.add(ToolInfo{
        .id = "annotate.dimaligned",
        .name = "Aligned Dimension",
        .category = "Annotate",
        .group = "Dimensions",
        .order = 2,
        .aliases = {"DIMALIGNED", "DAL"},
        .shortcut = {},
        .tip = "Dimensions the true distance between two points, with the dimension line "
               "parallel to them.",
        .make = annotate::makeAlignedDimensionTool,
    }));
    report(catalog.add(ToolInfo{
        .id = "annotate.leader",
        .name = "Leader",
        .category = "Annotate",
        .group = "Leaders",
        .order = 1,
        .aliases = {"LEADER", "LEAD", "LE"},
        .shortcut = {},
        .tip = "Draws an arrow to a feature with a note at its end: pick the tip and the bends, "
               "press Enter, then type the note.",
        .make = annotate::makeLeaderTool,
    }));
}

} // namespace katana::cad::tools
