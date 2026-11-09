//! The tool families (src/katana_cad/tools/families.hpp): each is one module
//! of `tools` that adds its tools to the catalogue (interactive_tool.rs). The
//! list is explicit rather than self-registering on purpose, as in the C++
//! (where a static archive drops an object file nothing references, taking
//! its tools with it): FAMILIES names every family, so none can go missing.
//!
//! A family adds each tool with `report(catalog.add(info))`, so a refused
//! tool is recorded rather than lost.
//!
//! # Adding a family (the cad port's wave I)
//!
//! A family's module (`tools::draw_lines`, ...) defines
//! `pub fn add_draw_line_tools(catalog: &mut ToolCatalog, report: &mut Report<'_>)`
//! and adds ONE line to FAMILIES below, in its place in the C++'s order
//! (interactive_tool.cpp's builtCatalog), which the integrator keeps when the
//! agents' lines meet:
//!
//! ```text
//! crate::tools::draw_lines::add_draw_line_tools,
//! crate::tools::draw_curves::add_draw_curve_tools,
//! crate::tools::modify_transform::add_modify_transform_tools,
//! crate::tools::modify_edit::add_modify_edit_tools,
//! crate::tools::annotate::add_annotate_tools,
//! crate::tools::inquiry::add_inquiry_tools,
//! crate::tools::draw_divide::add_draw_divide_tools,
//! crate::tools::modify_length::add_modify_length_tools,
//! crate::tools::modify_properties::add_property_tools,
//! crate::tools::select_tools::add_select_tools,
//! crate::tools::modify_vertex::add_modify_vertex_tools,
//! crate::tools::draw_professional::add_draw_professional_tools,
//! crate::tools::terrain_edit::add_terrain_edit_tools,
//! ```
//!
//! The C++ header also declares sketchMoveForTests, the Sketch tool's pen
//! moving through a point for the tests, which have no mouse; it is
//! draw_professional's (it reaches the tool through
//! InteractiveTool::as_any_mut, the C++'s dynamic_cast), and that family's
//! agent re-exports it here with a `pub use` line.

use katana_core::Status;

use crate::interactive_tool::{
    InteractiveTool, ToolCatalog, ToolContext, ToolInput, ToolStep, ToolStepOutcome,
};

/// What a family reports each tool's registration through (the C++ Report,
/// a `std::function<void(Status)>`).
pub type Report<'a> = dyn FnMut(Status) + 'a;

/// A family's adder: `fn(catalog: &mut ToolCatalog, report: &mut Report<'_>)`.
pub type Family = for<'a, 'b, 'c> fn(&'a mut ToolCatalog, &'b mut Report<'c>);

/// Every family, in the C++'s order (module doc).
pub const FAMILIES: &[Family] = &[
    crate::tools::draw_lines::add_draw_line_tools,
    crate::tools::draw_curves::add_draw_curve_tools,
    crate::tools::modify_transform::add_modify_transform_tools,
    crate::tools::modify_edit::add_modify_edit_tools,
    crate::tools::annotate::add_annotate_tools,
    crate::tools::inquiry::add_inquiry_tools,
    crate::tools::draw_divide::add_draw_divide_tools,
    crate::tools::modify_length::add_modify_length_tools,
    crate::tools::modify_properties::add_property_tools,
    crate::tools::select_tools::add_select_tools,
    crate::tools::modify_vertex::add_modify_vertex_tools,
    crate::tools::draw_professional::add_draw_professional_tools,
    crate::tools::terrain_edit::add_terrain_edit_tools,
];

/// The Sketch tool's pen moving through a point, for the tests (module doc).
pub use crate::tools::draw_professional::sketch_move_for_tests;

/// Esc for a tool whose Enter only ever commits work it has collected - a
/// LINE or PLINE chain, the cuts of a Trim or Extend, Offset's copies, a run
/// of Fillets or Chamfers: Enter, first stepped back out of any value prompt
/// (whose Enter would take a default), keeping a Done step's command and
/// dropping anything else. Such a tool's cancel() returns this.
pub fn keep_work_on_escape<T: InteractiveTool + ?Sized>(
    tool: &mut T,
    cx: &ToolContext<'_>,
) -> ToolStep {
    // Out of a value prompt first. Enter there takes the prompt's default -
    // at Chamfer's second distance it stores both distances for every later
    // Chamfer - or only returns to the picks (Fillet's radius), and then the
    // corners a Multiple session made would go with the tool. Undo at those
    // prompts only steps back towards the picks (CornerTool::undo); a value
    // prompt still showing after that is left rather than defaulted.
    const MOST_STEPS_BACK: i32 = 4; // SecondDistance -> FirstDistance -> First is 2
    let mut stepped = 0;
    while stepped < MOST_STEPS_BACK && tool.expects(cx) == ToolInput::Value {
        if tool.undo(cx).outcome != ToolStepOutcome::Continue {
            break;
        }
        stepped += 1;
    }
    if tool.expects(cx) == ToolInput::Value {
        return ToolStep::done(None, "", false);
    }
    let mut step = tool.enter(cx);
    // Only a finished step's command is work to keep. A Continue is Enter
    // moving the tool on a step (Trim's edges, Offset's distance) and a
    // Rejected changed nothing; either way there is nothing to keep.
    if step.outcome != ToolStepOutcome::Done {
        return ToolStep::done(None, "", false);
    }
    step.restart = false;
    step
}
