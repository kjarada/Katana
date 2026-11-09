//! Text and dimensions (src/katana_cad/tools/annotate.cpp; tools/families.rs).
//! The tools themselves are in annotate_text, annotate_dimension,
//! annotate_dimension_kinds, annotate_dimension_chain, annotate_leader,
//! annotate_balloon, annotate_label and annotate_leader_presets; this module
//! lists them in the catalogue.
//!
//! Aliases are AutoCAD's: TEXT and DTEXT/DT for single-line text,
//! DIMLINEAR/DLI, DIMALIGNED/DAL, and LEADER with both LEAD (AutoCAD's alias
//! for LEADER) and LE (QLEADER's, the leader most users now reach for). T is
//! left to the command interpreter, where it already means TEXT with its
//! arguments typed on one line, and DIM likewise stays the interpreter's,
//! every kind typed on one line ("DIM p p offset", "DIM RADIUS id at=p").
//!
//! Angular, radius, diameter and ordinate dimensions are AutoCAD's too:
//! DIMANGULAR/DAN, DIMRADIUS/DRA, DIMDIAMETER/DDI and DIMORDINATE/DOR. They
//! waited until the model could carry them (DimensionKind, 2026-09-25): drawn
//! as lines and a text they would have been dimensions in appearance only - a
//! label that stays "R5" when the arc is scaled to R10. Each is the dimension
//! the DIM verb makes (annotate_dimension_kinds).

use crate::annotation::leader_presets::LeaderPreset;
use crate::interactive_tool::{ToolCatalog, ToolFactory, ToolInfo};
use crate::tools::annotate_balloon::make_balloon_tool;
use crate::tools::annotate_common::factory;
use crate::tools::annotate_dimension::{make_aligned_dimension_tool, make_linear_dimension_tool};
use crate::tools::annotate_dimension_chain::{
    make_baseline_dimension_tool, make_continue_dimension_tool,
};
use crate::tools::annotate_dimension_kinds::{
    make_angular_dimension_tool, make_diameter_dimension_tool, make_ordinate_dimension_tool,
    make_radius_dimension_tool,
};
use crate::tools::annotate_label::make_label_tool;
use crate::tools::annotate_leader::make_leader_tool;
use crate::tools::annotate_leader_presets::make_preset_leader_tool;
use crate::tools::annotate_text::{make_multiline_text_tool, make_text_tool};
use crate::tools::families::Report;

// One Annotate tool as the catalogue lists it.
fn annotate(
    id: &str,
    name: &str,
    group: &str,
    order: i32,
    aliases: &[&str],
    tip: &str,
    make: ToolFactory,
) -> ToolInfo {
    ToolInfo {
        id: String::from(id),
        name: String::from(name),
        title: String::new(),
        category: String::from("Annotate"),
        group: String::from(group),
        order,
        aliases: aliases.iter().map(|&alias| String::from(alias)).collect(),
        shortcut: String::new(),
        tip: String::from(tip),
        make: Some(make),
    }
}

// A preset leader's factory: the Leader tool of `which` preset.
fn preset(which: LeaderPreset) -> ToolFactory {
    factory(move |cx, start| make_preset_leader_tool(cx, start, which))
}

/// Text and dimensions, leaders, balloons and labels: the Annotate menu.
pub fn add_annotate_tools(catalog: &mut ToolCatalog, report: &mut Report<'_>) {
    report(catalog.add(annotate(
        "annotate.text",
        "Text",
        "Text",
        1,
        &["TEXT", "DTEXT", "DT"],
        "Places single-line text: pick the start point (or S, J or P for the style, \
         justification and height on paper), give the height and rotation, then type each \
         line; an empty line finishes.",
        factory(make_text_tool),
    )));
    // MTEXT's text in one entity. MT and MTEXT alone start the tool; with
    // arguments they stay the interpreter's MTEXT verb, as TEXT does.
    report(catalog.add(annotate(
        "annotate.mtext",
        "Multiline Text",
        "Text",
        2,
        &["MTEXT", "MT"],
        "Places one text of several lines, paper-sized in the Standard style unless told \
         otherwise: pick the insertion point (or S, J or P), then type each line; an empty \
         line finishes.",
        factory(make_multiline_text_tool),
    )));
    report(catalog.add(annotate(
        "annotate.dimlinear",
        "Linear Dimension",
        "Dimensions",
        1,
        &["DIMLINEAR", "DLI"],
        "Dimensions the horizontal or vertical distance between two points, chosen by where \
         the dimension line is placed or by typing H or V.",
        factory(make_linear_dimension_tool),
    )));
    report(catalog.add(annotate(
        "annotate.dimaligned",
        "Aligned Dimension",
        "Dimensions",
        2,
        &["DIMALIGNED", "DAL"],
        "Dimensions the true distance between two points, with the dimension line parallel \
         to them.",
        factory(make_aligned_dimension_tool),
    )));
    report(catalog.add(annotate(
        "annotate.dimangular",
        "Angular Dimension",
        "Dimensions",
        3,
        &["DIMANGULAR", "DAN"],
        "Dimensions the angle between two lines, or at a vertex between two points; the \
         arc's place chooses the side measured.",
        factory(make_angular_dimension_tool),
    )));
    report(catalog.add(annotate(
        "annotate.dimradius",
        "Radius Dimension",
        "Dimensions",
        4,
        &["DIMRADIUS", "DRA"],
        "Dimensions the radius of an arc or a circle, and follows it when it is edited.",
        factory(make_radius_dimension_tool),
    )));
    report(catalog.add(annotate(
        "annotate.dimdiameter",
        "Diameter Dimension",
        "Dimensions",
        5,
        &["DIMDIAMETER", "DDI"],
        "Dimensions the diameter of an arc or a circle, and follows it when it is edited.",
        factory(make_diameter_dimension_tool),
    )));
    report(catalog.add(annotate(
        "annotate.dimordinate",
        "Ordinate Dimension",
        "Dimensions",
        6,
        &["DIMORDINATE", "DOR"],
        "Writes a feature's X or Y from a datum (the drawing's origin until Datum gives \
         another) at the end of a leader; the leader's direction chooses which, or type X or \
         Y.",
        factory(make_ordinate_dimension_tool),
    )));
    // AutoCAD's DIMBASELINE/DBA and DIMCONTINUE/DCO; DIM BASELINE and DIM
    // CONTINUE on the command line make the same chains
    // (annotate_dimension_chain).
    report(catalog.add(annotate(
        "annotate.dimbaseline",
        "Baseline Dimension",
        "Dimensions",
        7,
        &["DIMBASELINE", "DBA"],
        "Adds dimensions measured from the same first origin as the newest linear or aligned \
         dimension (or one chosen with Select), each a spacing further out: click each next \
         origin, then press Enter.",
        factory(make_baseline_dimension_tool),
    )));
    report(catalog.add(annotate(
        "annotate.dimcontinue",
        "Continue Dimension",
        "Dimensions",
        8,
        &["DIMCONTINUE", "DCO"],
        "Chains dimensions end to end from the newest linear or aligned dimension (or one \
         chosen with Select), on its dimension line: click each next origin, then press \
         Enter.",
        factory(make_continue_dimension_tool),
    )));
    report(catalog.add(annotate(
        "annotate.leader",
        "Leader",
        "Leaders",
        1,
        &["LEADER", "LEAD", "LE"],
        "Draws an arrow to a feature with a note at its end: pick the tip and the bends, \
         press Enter, then type the note; Arrow, Callout, Style and Paper change this \
         leader's look.",
        factory(make_leader_tool),
    )));
    // A bare BALLOON starts it; with arguments BALLOON is the interpreter's
    // verb, which makes the same balloon (annotate_balloon).
    report(catalog.add(annotate(
        "annotate.balloon",
        "Balloon",
        "Leaders",
        2,
        &["BALLOON"],
        "A numbered circle at the end of a leader, counting on from the drawing's highest \
         balloon: pick the tip and where the circle sits, then press Enter.",
        factory(make_balloon_tool),
    )));
    // The preset leaders (annotate_leader_presets): one tool a preset, so
    // each is one click on the toolbar - not a "Leader, ..." family, whose
    // toolbar button hides all but the first behind a drop-down. Each makes
    // what LEADER #id@x,y x,y preset=... makes; the word is the preset's.
    // The names leave the Annotate menu's letters room: each has one of its
    // own (V, U, F, P, O, S) and Text keeps T, not the X Edit Text takes
    // (tools/tool_menus.cpp, assignMnemonics; annotation_workbench.cpp).
    report(catalog.add(annotate(
        "annotate.leader.level",
        "Level Leader",
        "Leaders",
        3,
        &["LEADERLEVEL"],
        "A leader that says a point's level, RL to the millimetre, read live off the point: \
         click the point, then where the note goes (LEADER preset=level).",
        preset(LeaderPreset::Level),
    )));
    report(catalog.add(annotate(
        "annotate.leader.surface",
        "Surface Difference Leader",
        "Leaders",
        4,
        &["LEADERSURFACE"],
        "A leader that says how far a point is above (+) or below (-) a surface: click the \
         point, then where the note goes; S chooses the surface (LEADER preset=surface).",
        preset(LeaderPreset::Surface),
    )));
    report(catalog.add(annotate(
        "annotate.leader.cutfill",
        "Cut/Fill Leader",
        "Leaders",
        5,
        &["LEADERCUTFILL"],
        "A leader that says the cut down to a point below a surface, or the fill up to one \
         above it: click the point, then where the note goes; S chooses the surface (LEADER \
         preset=cutfill).",
        preset(LeaderPreset::CutFill),
    )));
    report(catalog.add(annotate(
        "annotate.leader.name",
        "Point Name Leader",
        "Leaders",
        6,
        &["LEADERNAME"],
        "A leader that says a point's name (its point number): click the point, then where \
         the note goes (LEADER preset=name).",
        preset(LeaderPreset::Name),
    )));
    report(catalog.add(annotate(
        "annotate.leader.code",
        "Code Leader",
        "Leaders",
        7,
        &["LEADERCODE"],
        "A leader that says the survey code of what it points at: click the point or the \
         line, then where the note goes (LEADER preset=code).",
        preset(LeaderPreset::Code),
    )));
    report(catalog.add(annotate(
        "annotate.leader.station",
        "Station Leader",
        "Leaders",
        8,
        &["LEADERSTATION"],
        "A leader that says the station along an alignment and the offset from it, left \
         positive: click the point, then where the note goes; A chooses the alignment \
         (LEADER preset=station).",
        preset(LeaderPreset::Station),
    )));
    // A bare LABEL starts it, as a bare LEADER starts the Leader tool; with
    // arguments LABEL is the interpreter's verb, which makes the same labels
    // (annotate_label).
    report(catalog.add(annotate(
        "annotate.label",
        "Label Objects",
        "Labels",
        1,
        &["LABELOBJECTS", "LBL", "LABEL"],
        "Labels the objects chosen in a label style - a point's number, a line's bearing and \
         distance, a lot's area: pick them, press Enter, then click where the text goes or \
         press Enter to let it find room.",
        factory(make_label_tool),
    )));
}
