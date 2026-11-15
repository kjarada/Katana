//! What a tool's preview says (include/katana/cad/tool_feedback.hpp;
//! interactive_tool's InteractiveTool::preview): the result as it will be
//! (the ghost), where the tool measures from, and - by ROLE - what in the
//! drawing already there a click here acts on, what it adds and what it takes
//! away. The owner's request of 2026-09-30: inserting a vertex gave "no visual
//! clue where the vertex is going". One dashed pen for everything could not
//! say "this segment splits", "the new vertex lands here" or "this vertex
//! goes"; a role can, and the view draws each role its own way (the window's
//! feedback painter), so a tool never chooses a colour.
//!
//! In a module of its own so that drawing/grips can build one for a grip's
//! drag without the interactive tool, which uses grips for a tool's handles
//! (ToolContext::handles).

use katana_entity::{EntityId, EnumName, Geometry};
use katana_geometry::Point2;

/// What a mark says about the drawing already there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum FeedbackRole {
    /// What a click here takes or acts on: the vertex, the segment, the
    /// polyline.
    #[default]
    Target,
    /// Where something new lands: a vertex read from the RESULT, not the
    /// cursor.
    Added,
    /// What the step takes away: a vertex deleted or straightened out, a
    /// piece cut.
    Removed,
    /// What ENTER, not a click here, acts on: Insert Vertex's place at the
    /// middle of a segment the cursor may not be on, and the vertices chosen
    /// before a tool that Enter deletes, straightens or makes the start while
    /// the pointer is over another. A place or a vertex Enter uses must be
    /// seen before it is pressed; with the pointer on another vertex the
    /// chosen one was drawn as a plain square, and "vertex 2" in the prompt
    /// could not be found on screen.
    Enter,
}

/// "target", "added", "removed", "enter": how the headless pointer record
/// names them - its keys are these strings (the viewport's pointer record).
/// The C++ `toString(FeedbackRole)`, which interactive_tool.cpp defines.
impl EnumName for FeedbackRole {
    fn name(self) -> &'static str {
        match self {
            Self::Target => "target",
            Self::Added => "added",
            Self::Removed => "removed",
            Self::Enter => "enter",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FeedbackMark {
    pub role: FeedbackRole,
    /// A PointGeometry is a VERTEX, drawn as a fixed-size glyph; anything
    /// else is a PIECE (a segment, an arc, a whole polyline), drawn along its
    /// geometry.
    pub geometry: Geometry,
    /// Beside a vertex glyph: its number, "keep", "z 101.500". Empty for
    /// none.
    pub label: String,
    /// This mark is WHY a click here is refused - the vertex a new one would
    /// be too near, the end that is no corner - or a place Enter would be
    /// refused at. The view draws it with the refusal's own shape, a ring
    /// struck through, as well as its colour: red and green alone are one
    /// khaki to a red-green colour-blind eye. When a refused preview flags no
    /// mark, every Target in it is drawn so (ToolFeedback::refused).
    pub refused: bool,
}

/// The rubber band: what the view draws for the current cursor position, in
/// model coordinates, over the drawing. Every member is optional; a tool that
/// fills only `shapes` and `markers` is drawn as every tool was before roles.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ToolFeedback {
    /// The result as it will be, dashed in one pen: what a click here makes.
    /// Only the pieces that change, never a whole polyline again.
    pub shapes: Vec<Geometry>,
    /// Points the tool measures from: a base point, a polygon's centre.
    pub markers: Vec<Point2>,
    /// What the preview says about the drawing already there, by role.
    pub marks: Vec<FeedbackMark>,
    /// One line beside the cursor: what a click (or Enter) here does - "new
    /// vertex between 1 and 2 · 15.000 from 1" - or, with `refused`, why it
    /// cannot.
    pub caption: String,
    /// A click here would be refused, for the reason `caption` gives; the
    /// view says so in the refusal's colour and draws the marks that say why
    /// in the refusal's shape - those flagged FeedbackMark::refused, or, when
    /// none is flagged, every Target, the pick that is turned down.
    pub refused: bool,
    /// The polyline in play, whose vertices the view shows while the tool
    /// runs (the grips are hidden then).
    pub focus: Option<EntityId>,
}
