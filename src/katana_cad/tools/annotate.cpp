// Text and dimensions (see families.hpp). The tools themselves are in
// annotate_text.cpp, annotate_dimension.cpp, annotate_dimension_kinds.cpp and
// annotate_leader.cpp; this file
// lists them, so the one call toolCatalog() makes keeps every one of them in
// the link.
//
// Aliases are AutoCAD's: TEXT and DTEXT/DT for single-line text, DIMLINEAR/DLI,
// DIMALIGNED/DAL, and LEADER with both LEAD (AutoCAD's alias for LEADER) and
// LE (QLEADER's, the leader most users now reach for). T is left to the
// command interpreter, where it already means TEXT with its arguments typed on
// one line, and DIM likewise stays the interpreter's, every kind typed on one
// line ("DIM p p offset", "DIM RADIUS id at=p").
//
// Angular, radius, diameter and ordinate dimensions are AutoCAD's too:
// DIMANGULAR/DAN, DIMRADIUS/DRA, DIMDIAMETER/DDI and DIMORDINATE/DOR. They
// waited until the model could carry them (DimensionKind, 2026-09-25): drawn
// as lines and a text they would have been dimensions in appearance only - a
// label that stays "R5" when the arc is scaled to R10. Each is the dimension
// the DIM verb makes (annotate_dimension_kinds.cpp).

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
        .tip = "Places single-line text: pick the start point (or S, J or P for the style, "
               "justification and height on paper), give the height and rotation, then type "
               "each line; an empty line finishes.",
        .make = annotate::makeTextTool,
    }));
    // MTEXT's text in one entity. MT and MTEXT alone start the tool; with
    // arguments they stay the interpreter's MTEXT verb, as TEXT does.
    report(catalog.add(ToolInfo{
        .id = "annotate.mtext",
        .name = "Multiline Text",
        .category = "Annotate",
        .group = "Text",
        .order = 2,
        .aliases = {"MTEXT", "MT"},
        .shortcut = {},
        .tip = "Places one text of several lines, paper-sized in the Standard style unless told "
               "otherwise: pick the insertion point (or S, J or P), then type each line; an "
               "empty line finishes.",
        .make = annotate::makeMultilineTextTool,
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
        .id = "annotate.dimangular",
        .name = "Angular Dimension",
        .category = "Annotate",
        .group = "Dimensions",
        .order = 3,
        .aliases = {"DIMANGULAR", "DAN"},
        .shortcut = {},
        .tip = "Dimensions the angle between two lines, or at a vertex between two points; the "
               "arc's place chooses the side measured.",
        .make = annotate::makeAngularDimensionTool,
    }));
    report(catalog.add(ToolInfo{
        .id = "annotate.dimradius",
        .name = "Radius Dimension",
        .category = "Annotate",
        .group = "Dimensions",
        .order = 4,
        .aliases = {"DIMRADIUS", "DRA"},
        .shortcut = {},
        .tip = "Dimensions the radius of an arc or a circle, and follows it when it is edited.",
        .make = annotate::makeRadiusDimensionTool,
    }));
    report(catalog.add(ToolInfo{
        .id = "annotate.dimdiameter",
        .name = "Diameter Dimension",
        .category = "Annotate",
        .group = "Dimensions",
        .order = 5,
        .aliases = {"DIMDIAMETER", "DDI"},
        .shortcut = {},
        .tip = "Dimensions the diameter of an arc or a circle, and follows it when it is edited.",
        .make = annotate::makeDiameterDimensionTool,
    }));
    report(catalog.add(ToolInfo{
        .id = "annotate.dimordinate",
        .name = "Ordinate Dimension",
        .category = "Annotate",
        .group = "Dimensions",
        .order = 6,
        .aliases = {"DIMORDINATE", "DOR"},
        .shortcut = {},
        .tip = "Writes a feature's X or Y from the drawing's origin at the end of a leader; the "
               "leader's direction chooses which, or type X or Y.",
        .make = annotate::makeOrdinateDimensionTool,
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
