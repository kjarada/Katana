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
    // The preset leaders (annotate_leader_presets.cpp): one tool a preset, so
    // each is one click on the toolbar - not a "Leader, ..." family, whose
    // toolbar button hides all but the first behind a drop-down. Each makes
    // what LEADER #id@x,y x,y preset=... makes; the word is the preset's.
    // The names leave the Annotate menu's letters room: each has one of its
    // own (V, U, F, P, O, S) and Text keeps T, not the X Edit Text takes
    // (tools/tool_menus.cpp, assignMnemonics; annotation_workbench.cpp).
    const auto preset = [](katana::cad::annotation::LeaderPreset which) {
        return [which](const ToolContext& context) {
            return annotate::makePresetLeaderTool(context, which);
        };
    };
    using katana::cad::annotation::LeaderPreset;
    report(catalog.add(ToolInfo{
        .id = "annotate.leader.level",
        .name = "Level Leader",
        .category = "Annotate",
        .group = "Leaders",
        .order = 3,
        .aliases = {"LEADERLEVEL"},
        .shortcut = {},
        .tip = "A leader that says a point's level, RL to the millimetre, read live off the point: "
               "click the point, then where the note goes (LEADER preset=level).",
        .make = preset(LeaderPreset::Level),
    }));
    report(catalog.add(ToolInfo{
        .id = "annotate.leader.surface",
        .name = "Surface Difference Leader",
        .category = "Annotate",
        .group = "Leaders",
        .order = 4,
        .aliases = {"LEADERSURFACE"},
        .shortcut = {},
        .tip = "A leader that says how far a point is above (+) or below (-) a surface: click the "
               "point, then where the note goes; S chooses the surface (LEADER preset=surface).",
        .make = preset(LeaderPreset::Surface),
    }));
    report(catalog.add(ToolInfo{
        .id = "annotate.leader.cutfill",
        .name = "Cut/Fill Leader",
        .category = "Annotate",
        .group = "Leaders",
        .order = 5,
        .aliases = {"LEADERCUTFILL"},
        .shortcut = {},
        .tip = "A leader that says the cut down to a point below a surface, or the fill up to one "
               "above it: click the point, then where the note goes; S chooses the surface "
               "(LEADER preset=cutfill).",
        .make = preset(LeaderPreset::CutFill),
    }));
    report(catalog.add(ToolInfo{
        .id = "annotate.leader.name",
        .name = "Point Name Leader",
        .category = "Annotate",
        .group = "Leaders",
        .order = 6,
        .aliases = {"LEADERNAME"},
        .shortcut = {},
        .tip = "A leader that says a point's name (its point number): click the point, then where "
               "the note goes (LEADER preset=name).",
        .make = preset(LeaderPreset::Name),
    }));
    report(catalog.add(ToolInfo{
        .id = "annotate.leader.code",
        .name = "Code Leader",
        .category = "Annotate",
        .group = "Leaders",
        .order = 7,
        .aliases = {"LEADERCODE"},
        .shortcut = {},
        .tip = "A leader that says the survey code of what it points at: click the point or the "
               "line, then where the note goes (LEADER preset=code).",
        .make = preset(LeaderPreset::Code),
    }));
    report(catalog.add(ToolInfo{
        .id = "annotate.leader.station",
        .name = "Station Leader",
        .category = "Annotate",
        .group = "Leaders",
        .order = 8,
        .aliases = {"LEADERSTATION"},
        .shortcut = {},
        .tip = "A leader that says the station along an alignment and the offset from it, left "
               "positive: click the point, then where the note goes; A chooses the alignment "
               "(LEADER preset=station).",
        .make = preset(LeaderPreset::Station),
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
