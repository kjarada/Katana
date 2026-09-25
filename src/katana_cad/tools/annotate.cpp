// Text and dimensions (see families.hpp). The tools themselves are in
// annotate_text.cpp, annotate_dimension.cpp, annotate_dimension_kinds.cpp,
// annotate_dimension_chain.cpp, annotate_leader.cpp, annotate_balloon.cpp and
// annotate_label.cpp; this file
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
        .tip = "Writes a feature's X or Y from a datum (the drawing's origin until Datum gives "
               "another) at the end of a leader; the leader's direction chooses which, or type "
               "X or Y.",
        .make = annotate::makeOrdinateDimensionTool,
    }));
    // AutoCAD's DIMBASELINE/DBA and DIMCONTINUE/DCO; DIM BASELINE and DIM
    // CONTINUE on the command line make the same chains
    // (annotate_dimension_chain.cpp).
    report(catalog.add(ToolInfo{
        .id = "annotate.dimbaseline",
        .name = "Baseline Dimension",
        .category = "Annotate",
        .group = "Dimensions",
        .order = 7,
        .aliases = {"DIMBASELINE", "DBA"},
        .shortcut = {},
        .tip = "Adds dimensions measured from the same first origin as the newest linear or "
               "aligned dimension (or one chosen with Select), each a spacing further out: click "
               "each next origin, then press Enter.",
        .make = annotate::makeBaselineDimensionTool,
    }));
    report(catalog.add(ToolInfo{
        .id = "annotate.dimcontinue",
        .name = "Continue Dimension",
        .category = "Annotate",
        .group = "Dimensions",
        .order = 8,
        .aliases = {"DIMCONTINUE", "DCO"},
        .shortcut = {},
        .tip = "Chains dimensions end to end from the newest linear or aligned dimension (or one "
               "chosen with Select), on its dimension line: click each next origin, then press "
               "Enter.",
        .make = annotate::makeContinueDimensionTool,
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
               "press Enter, then type the note; Arrow, Callout, Style and Paper change this "
               "leader's look.",
        .make = annotate::makeLeaderTool,
    }));
    // A bare BALLOON starts it; with arguments BALLOON is the interpreter's
    // verb, which makes the same balloon (annotate_balloon.cpp).
    report(catalog.add(ToolInfo{
        .id = "annotate.balloon",
        .name = "Balloon",
        .category = "Annotate",
        .group = "Leaders",
        .order = 2,
        .aliases = {"BALLOON"},
        .shortcut = {},
        .tip = "A numbered circle at the end of a leader, counting on from the drawing's highest "
               "balloon: pick the tip and where the circle sits, then press Enter.",
        .make = annotate::makeBalloonTool,
    }));
    // A bare LABEL starts it, as a bare LEADER starts the Leader tool; with
    // arguments LABEL is the interpreter's verb, which makes the same labels
    // (annotate_label.cpp).
    report(catalog.add(ToolInfo{
        .id = "annotate.label",
        .name = "Label Objects",
        .category = "Annotate",
        .group = "Labels",
        .order = 1,
        .aliases = {"LABELOBJECTS", "LBL", "LABEL"},
        .shortcut = {},
        .tip = "Labels the objects chosen in a label style - a point's number, a line's bearing "
               "and distance, a lot's area: pick them, press Enter, then click where the text "
               "goes or press Enter to let it find room.",
        .make = annotate::makeLabelTool,
    }));
}

} // namespace katana::cad::tools
