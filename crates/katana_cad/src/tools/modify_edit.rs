//! Trim, Extend, Offset, Fillet, Chamfer, Break, Break at Point, Join,
//! Explode (src/katana_cad/tools/modify_edit.cpp; see families.rs): the
//! Modify menu's Edit group.
//!
//! Every tool here builds its whole operation in an EditSession
//! (modify_edit_support.rs) and hands the document ONE command when it
//! finishes, so one undo takes back everything the tool did - all the picks
//! of a Trim, all the corners of a Multiple fillet. The session is also why
//! these tools do not return trim_entity, fillet_entities and their like
//! directly: a second pick can land on a piece the first one made, which has
//! no id until a command runs, and a fillet must keep the side of each line
//! the user picked, which fillet_entities does not know. The geometry is the
//! same functions those commands use (katana_geometry's editing).

use std::cell::RefCell;
use std::rc::Rc;

use crate::interactive_tool::{ToolCatalog, ToolContext, ToolFactory, ToolInfo, ToolStart};
use crate::tools::families::Report;
use crate::tools::modify_edit_support::{
    Defaults, EditDefaults, ToolPtr, make_break_at_point_tool, make_break_tool, make_chamfer_tool,
    make_explode_tool, make_extend_tool, make_fillet_tool, make_join_tool, make_offset_tool,
    make_trim_tool, tool_factory,
};

thread_local! {
    // One set of remembered values for the program, as AutoCAD keeps
    // FILLETRAD and OFFSETDIST: the C++ catalogue is built once, so its
    // factories share one object for as long as it runs. The program runs
    // its tools on one thread, so the set is that thread's (cad_plan.md
    // section 7, risk 4).
    static EDIT_DEFAULTS: Defaults = Rc::new(RefCell::new(EditDefaults::default()));
}

fn edit_defaults() -> Defaults {
    EDIT_DEFAULTS.with(Rc::clone)
}

fn trim(_cx: &ToolContext<'_>, start: &ToolStart) -> ToolPtr {
    make_trim_tool(start)
}

fn extend(_cx: &ToolContext<'_>, start: &ToolStart) -> ToolPtr {
    make_extend_tool(start)
}

fn offset(_cx: &ToolContext<'_>, _start: &ToolStart) -> ToolPtr {
    make_offset_tool(edit_defaults())
}

fn fillet(_cx: &ToolContext<'_>, _start: &ToolStart) -> ToolPtr {
    make_fillet_tool(edit_defaults())
}

fn chamfer(_cx: &ToolContext<'_>, _start: &ToolStart) -> ToolPtr {
    make_chamfer_tool(edit_defaults())
}

fn break_two_points(_cx: &ToolContext<'_>, _start: &ToolStart) -> ToolPtr {
    make_break_tool()
}

fn break_at_point(_cx: &ToolContext<'_>, _start: &ToolStart) -> ToolPtr {
    make_break_at_point_tool()
}

fn join(_cx: &ToolContext<'_>, start: &ToolStart) -> ToolPtr {
    make_join_tool(start, edit_defaults())
}

fn explode(_cx: &ToolContext<'_>, start: &ToolStart) -> ToolPtr {
    make_explode_tool(start)
}

pub fn add_modify_edit_tools(catalog: &mut ToolCatalog, report: &mut Report<'_>) {
    let mut add =
        |id: &str, name: &str, order: i32, aliases: &[&str], tip: &str, make: ToolFactory| {
            let info = ToolInfo {
                id: String::from(id),
                name: String::from(name),
                category: String::from("Modify"),
                group: String::from("Edit"),
                order,
                aliases: aliases.iter().map(|&alias| String::from(alias)).collect(),
                tip: String::from(tip),
                make: Some(make),
                ..ToolInfo::default()
            };
            report(catalog.add(info));
        };

    add(
        "modify.trim",
        "Trim",
        10,
        &["TRIM", "TR"],
        "Cuts objects back to cutting edges: pick the edges and press Enter (or Enter at once \
         for every object), then pick each part to remove.",
        tool_factory(trim),
    );
    add(
        "modify.extend",
        "Extend",
        20,
        &["EXTEND", "EX"],
        "Lengthens lines, arcs and open polylines to boundary edges: pick the boundaries and \
         press Enter (or Enter at once for every object), then pick near each end to extend.",
        tool_factory(extend),
    );
    add(
        "modify.offset",
        "Offset",
        30,
        &["OFFSET", "O"],
        "Makes a parallel copy of a line, arc, circle or polyline: type the distance (or T to \
         go through a point), pick the object, then click the side; repeat, Enter to finish.",
        tool_factory(offset),
    );
    add(
        "modify.fillet",
        "Fillet",
        40,
        &["FILLET", "F"],
        "Rounds the corner between two lines with an arc of the current radius (R to change \
         it; radius 0 makes a sharp corner), keeping the side of each line you pick.",
        tool_factory(fillet),
    );
    add(
        "modify.chamfer",
        "Chamfer",
        50,
        &["CHAMFER", "CHA"],
        "Cuts the corner between two lines with a bevel set back by the two chamfer distances \
         (D to change them), the first distance along the first line picked.",
        tool_factory(chamfer),
    );
    add(
        "modify.break",
        "Break",
        60,
        &["BREAK", "BR"],
        "Removes the part of a line, arc, circle or polyline between two points: pick the \
         object at the first point (or F to give it), then the second; @0,0 breaks without a \
         gap.",
        tool_factory(break_two_points),
    );
    add(
        "modify.break_at_point",
        "Break at Point",
        70,
        &["BREAKATPOINT"],
        "Splits a line, arc or open polyline in two at one point: pick the object, then the \
         point.",
        tool_factory(break_at_point),
    );
    add(
        "modify.join",
        "Join",
        80,
        &["JOIN", "J"],
        "Joins lines and open polylines whose ends meet (within the stated tolerance) into one \
         polyline; arcs are left out, as a polyline has straight segments only.",
        tool_factory(join),
    );
    add(
        "modify.explode",
        "Explode",
        90,
        &["EXPLODE", "X"],
        "Takes polylines, rectangles included, apart into their individual lines.",
        tool_factory(explode),
    );
}
